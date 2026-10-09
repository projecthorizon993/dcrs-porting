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
| [`dcrs-core`](crates/dcrs-core) | Headless client core: gateway session state machine, event bus, cache, rate limiter, credential storage. No UI, no sockets. |
| [`dcrs-theme`](crates/dcrs-theme) | CSS subset parser, variable resolution, class-map translation, and a shadow tree so selectors have something to match. |
| [`dcrs-compat`](crates/dcrs-compat) | The capability registry: every surface, its class, and whether it is implemented. |
| [`dcrs-plugins`](crates/dcrs-plugins) | The native plugin ABI. Manifests mirror Vencord's `PluginDef` field-for-field. |
| [`dcrs-port`](crates/dcrs-port) | The CLI. Analyzes a theme or plugin and prints a portability verdict. |

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

```console
$ dcrs-port --registry assets/capabilities.toml coverage
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
$ dcrs-port --registry assets/capabilities.toml plugin examples/example-plugin.ts
plugin: ExamplePlugin
  definePlugin detected: true
  (a) data read  [3]
  (a) stores.ChannelStore                not-implemented  line 33
  (b) data write  [3]
  (b) flux.dispatch                      not-implemented  line 37
  (d) ui injection  [2]
  (d) ui.renderMessageAccessory          not-implemented  line 60
  (e) internals patch  [2]
  (e) internals.patcherBefore            not-replicable   line 47
  (e) patches:[] array with ~2 entries          NOT REPLICABLE
  blocker: yes - needs a native rewrite or cannot be ported
  verdict: NOT PORTABLE AS-IS - declarative patches target the minified bundle
```

```console
$ dcrs-port --class-map assets/class-map.json theme examples/example-theme.css
theme: Example Theme
  author: dcrs
  variables declared 12 / resolved 12
  tier mix
    variables      2
    var-refs       1
    prefix         2
    hashed         2
    structural     2
  class map     2/2 hashed selectors mapped
  geometry rules 1
  verdict: MOSTLY FULL - variable overrides work; geometry rules need manual work
```

Add `--json` to any subcommand for machine-readable output.

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

### Mods: the five classes

`dcrs-compat` classifies every surface. The rule is simple and enforced by the loader:

- `internals` surfaces may be `unsupported`. Replicable classes may not — a surface that is merely
  unbuilt is a tracking gap, not a ceiling.
- `Registry::coverage()` excludes internals from its denominator, so 100% is reachable.

## Development

```console
cargo test --workspace     # 185 tests
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

`unsafe_code` is forbidden workspace-wide; clippy runs at `pedantic`.

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
