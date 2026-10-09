# dcrs

Tooling for porting Discord client **themes** and **mods** (BetterDiscord / Vencord / Equicord /
Shelter) to a **native** Rust Discord client.

> **Status: Phase 0.** This repository is the porting infrastructure, not a Discord client. The
> client it targets is planned in [`PORT-PLAN.md`](PORT-PLAN.md); the design that motivates these
> crates is in [`PORT-PLAN.md` § Part 2](PORT-PLAN.md#part-2--theme--mod-porting-system).

## Why this exists

A native client has no web app, so **both** of Legcord's headline features die by default: JS mods
patch a minified bundle they cannot own, and BetterDiscord themes target obfuscated class names.

A source survey of the ecosystem (`Vendicated/Vencord`, `BetterDiscord`, `uwu/shelter`, plus four
real theme corpora) produced a more useful answer than "you lose them":

| Finding | Consequence |
|---|---|
| Mod APIs split into 5 classes: read, write, subscription, UI injection, internals patch | ~80% of the surface is a clean contract. **UI injection is free** — a native client owns the render tree. |
| Internals patching is string-matching against a minified bundle | **Not replicable, ever.** Depends on an artifact we do not control. |
| Maintained themes are **98.7% hashed-class selectors** (1,823 of 1,848 in ClearVision v7) | The burden is *translating one naming scheme into another*, not parsing. |
| ClearVision v7 keeps all 3,198 hashed names in one generated Sass map | We do the same in JSON, and the tool reports the unmapped remainder as a work queue. |
| ClearVision's **shipped** CSS is 8.6 KB with zero hashed selectors | Most of that corpus compiles down to `:root` variable overrides. |
| Legacy themes use `[class*="prefix"]` (~1,560 uses) | Works unchanged if the UI emits stable prefixes like `channel-`. |
| `data-*` is the ecosystem's own escape hatch (`ThemeAttributes`, `SurfaceClasses`) | The shadow tree publishes exactly those attributes. |

**Headline:** ~80% of the mod API and ~95% of shipped theme CSS are portable. The irreducible loss
is internals patching.

## Crates

| Crate | What it does |
|---|---|
| [`dcrs-core`](crates/dcrs-core) | Headless client core: gateway session state machine, event bus, cache, rate limiter, credential storage. No UI, no sockets by default. |
| [`dcrs-theme`](crates/dcrs-theme) | CSS subset parser, variable resolution, class-map translation, and a shadow tree so selectors have something to match. |
| [`dcrs-compat`](crates/dcrs-compat) | The capability registry: every surface, its class, and whether it is implemented. Plus a separate media axis — a client can speak every codec a mod asks for and still have nothing for a media plugin to land on. |
| [`dcrs-serein`](crates/dcrs-serein) | Serein's theme schema as typed Rust, and the projection of a theme's CSS variables onto its 18 colour tokens and 15 control metrics. |
| [`dcrs-plugins`](crates/dcrs-plugins) | The native plugin ABI. Manifests mirror Vencord's `PluginDef` field-for-field. |
| [`dcrs-port`](crates/dcrs-port) | The CLI. Analyzes a theme or plugin and prints a portability verdict, by API surface, by effect, or by media; and converts a theme into a native Serein package. |

### `dcrs-core`

Nothing in it opens a socket. The gateway sits behind a `GatewayTransport` trait, so the whole
connect → identify → heartbeat → resume → reconnect cycle is tested against a mock with no network
and no token.

```rust
use dcrs_core::{Backoff, IdentifyConfig, Inbound, Session, SessionConfig};

let mut session = Session::new(SessionConfig::new(IdentifyConfig::new("token", 402_402)));

// Discord sends Hello first.
let step = session.on_frame(&Inbound::hello(41_250));
assert!(matches!(step.sent[0], dcrs_core::Outbound::Identify { .. }));

// Ready gives us what a later reconnect needs to resume.
session.on_frame(&Inbound::dispatch(
    "READY",
    1,
    serde_json::json!({ "session_id": "abc", "user": { "id": "42" } }),
));
assert!(session.can_resume());
```

Notable details:

- **Dispatch frames have no `op` field.** Discord omits it, so the parser matches on the event
  name instead.
- **Snowflakes serialize as strings**, because they exceed JavaScript's 53-bit safe integer range.
- **Backoff uses full jitter** (`random(0, min(cap, base·2ⁿ))`) so clients dropped by a gateway
  deploy don't reconnect in lockstep.
- **`Credential` redacts its own `Debug`.** Redaction lives on the credential, not on a wrapper,
  because a wrapper is one refactor away from being bypassed.
- **The cache is `Arc<RwLock<..>>`.** Clones are cheap and the UI can read from another thread.

## Usage

The capability registry and class map are **compiled into the binary**, so a downloaded release needs
no data files beside it. Pass `--registry` or `--class-map` only to test a different or edited copy.

```console
$ dcrs-port coverage
capability registry
  surfaces   62
  coverage   14.9% of replicable surfaces
  by class
    (a) data read          19  replicable
    (b) data write          6  replicable
    (c) subscription        8  replicable
    (d) ui injection       14  replicable
    (e) internals patch    15  NOT replicable
```

```console
$ dcrs-port plugin examples/example-plugin.ts
plugin: ExamplePlugin
  definePlugin detected: true
  (a) data read  [3]
  (a) stores.ChannelStore                partial          line 33
  (a) stores.GuildMemberStore            partial          line 34
  (a) stores.MediaEngineStore            not-implemented  line 41
  (b) data write  [3]
  (b) dataStore.del                      not-implemented  line 55
  (b) dataStore.set                      not-implemented  line 51
  (b) flux.dispatch                      not-implemented  line 37
  (d) ui injection  [2]
  (d) ui.chatBarButton                   not-implemented  line 64
```

```console
$ dcrs-port theme examples/example-theme.css
theme: Example Theme
  author: dcrs
  variables declared 15 / resolved 11
  tier mix
    variables    5
    var-refs     0
    prefix       2
    hashed       2
    structural   2
  class map     3/3 hashed selectors mapped
  geometry rules 1
  verdict: MOSTLY FULL - variable overrides work; geometry rules need manual work
```

```console
$ dcrs-port convert examples/example-theme.css
theme: Example Theme
  author: dcrs
  tokens filled 18/36 (50%)
  dropped: nothing
wrote examples/example-theme.serein-extension
```

Add `--json` to any subcommand for machine-readable output. `effects` and `media` exit non-zero when a
plugin does something that does not port, so they can gate a pipeline rather than only inform one.

## Design

### Theming: tiers

| Tier | Technique | Mechanism |
|---|---|---|
| T1 | `:root` / `.theme-dark` variable declarations | direct token resolution |
| T2 | `var()` layering, `--x-hsl` triples, `calc()` | variable graph resolution |
| T3 | `[class*="prefix"]` | works if the UI emits stable prefixes |
| T4 | fully hashed classes | `class-map.json` translation table |
| T5 | `:has()`, `:nth-child()`, `aria-*` | the shadow tree |
| T6 | geometry, `::before` art, custom fonts | **out of scope** — no CSS box model |

This crate resolves **values**, not layout. A native immediate-mode GUI has no box model, so
`width`, `position`, `transform` and negative margins cannot be honoured. `dcrs-port theme` counts
those rules explicitly rather than pretending they work.

### Mods: two axes, not one

Classifying by API call answers "can this plugin call that API natively", which for a mod like
FakeNitro answers *no to everything* — it is entirely webpack patches. That answer is useless for
planning, because the user-visible effect is perfectly portable.

So there are two independent questions:

| Axis | Question | FakeNitro |
|---|---|---|
| **API surface** (`dcrs-compat`) | can this call be made natively? | no — 11 patches, all class (e) |
| **Effect** (`dcrs-compat::Capability`) | what does the user actually get? | 14 capability flags + a message-rewrite pass |

`dcrs-port effects` reports the second axis. Every FakeNitro patch overrides a boolean gate and
replaces it with `return true` — `canStreamQuality`, `canUseClientThemes`, `canUsePremiumAppIcons`,
`available` on stickers and soundboard sounds. Those are fields in a struct a native client owns,
not patches.

```console
$ dcrs-port effects examples/fake-nitro.ts
  capability gates (14 native flags)
    high-quality streaming             -> Capabilities::stream_quality
      overrides canStreamQuality; conditional on settings.store.enableStreamQualityBypass
    client themes                      -> Capabilities::client_themes
      overrides canUseClientThemes
    role-subscription emojis           -> Capabilities::emoji_gate
      overrides GUILD_SUBSCRIPTION_UNAVAILABLE
      note: local only: other participants still see a plain link
  ...
  verdict: fully implementable as native capability flags
```

Emoji capabilities carry an explicit caveat: Discord renders the custom emoji for other people from
its own data, so a local override cannot be made to look the same for everyone. The tool states
that rather than glossing over it.

### Registry invariants

- `internals` surfaces may be `unsupported`. Replicable classes may not — a surface that is merely
  unbuilt is a tracking gap, not a ceiling.
- `Registry::coverage()` excludes internals from its denominator, so 100% is reachable.

## Backends

`dcrs-core` has **no required gateway dependency**. `default = []` compiles the protocol, session
machine, cache and event bus with no TLS stack and no sockets; I/O is opt-in:

```toml
dcrs-core = { version = "0.1", features = ["websocket"] }   # built-in tungstenite transport
dcrs-core = { version = "0.1", features = ["compression"] } # zlib-stream framing only
dcrs-core = { version = "0.1", features = ["rest", "tls-rustls"] }
```

Any Rust Discord library works instead, behind the `GatewayTransport` trait:
`discord_client_gateway`, `discordrs`, `twilight`, `serenity`. If it hands you a raw event name and
a `serde_json::Value` — all of them do — `decode_event` does the rest. See
[`crates/dcrs-core/src/backend.rs`](crates/dcrs-core/src/backend.rs).

`discord_client_gateway` is the one worth reaching for: it is user-mode (a `capabilities`
bitfield rather than bot intents) and ships Chrome TLS/HTTP2 impersonation, which is what a native
client needs to avoid looking like one.

## Development

```console
cargo test --workspace                 # no features
cargo test --workspace --all-features  # includes the websocket transport
cargo clippy --workspace --all-targets --all-features
cargo fmt --all --check
```

`unsafe_code` is forbidden workspace-wide; clippy runs at `pedantic`.

### Releases

Automatic. Bump `version` in the root `Cargo.toml`, commit, push — a release appears. There is no tag
to create and nothing to click.

```console
# edit version = "0.3.0" in Cargo.toml, then
git commit -am "Release 0.3.0"
git push
```

Automatic does not mean every commit ships one. Publishing on every push would put a public release
behind each typo fix, so the workflow compares `Cargo.toml`'s version against the latest release and
does nothing when they match. The trigger is automatic; the decision to release is still the version
number, which is the thing worth deciding on.

The version is read from `Cargo.toml` and never from a tag, so a binary cannot claim a version it was
not built from. Re-running is idempotent — same version, same release — and `overwrite_files` means a
second attempt replaces the first attempt's binaries rather than sitting beside them.

`dcrs-port` is built for Linux x86-64, Windows x86-64, and macOS Apple Silicon, uploaded as bare
executables with no archive in the way.

**Each artifact is executed before it is uploaded**, and that is not ceremony. The capability registry
and class map are `include_str!`d into the binary rather than shipped beside it, and an earlier build
compiled cleanly while shipping no data at all — `coverage` reported zero surfaces, which reads exactly
like a real answer. The workflow now asserts the registry is non-empty, that a conversion produces a
package with the manifest the host requires, and that a hashed selector still translates.

### Toolchain note

On Windows, the MSVC toolchain needs Visual Studio Build Tools for `link.exe`. The GNU toolchain
plus MSYS2 works without it:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
winget install --id MSYS2.MSYS2
& C:\msys64\usr\bin\bash -lc "pacman -S --noconfirm mingw-w64-ucrt-x86_64-binutils mingw-w64-ucrt-x86_64-gcc"
$env:PATH = "C:\msys64\ucrt64\bin;$env:PATH"
```

## Scope and honesty

- **Themes** are values, not layout. T6 is genuinely not portable.
- **Mods** that only use stores, events, settings and UI slots port. Mods that patch internals do
  not. `dcrs-port` says which, per plugin, instead of leaving it to guesswork.
- This is tooling for a client that does not exist yet. It is deliberately built against the
  capability registry so the two can be developed in parallel.
- Writing a Discord client violates Discord's Terms of Service. See `PORT-PLAN.md` § 0.

## License

MIT. The research and design are in [`PORT-PLAN.md`](PORT-PLAN.md).
