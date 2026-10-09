# Legcord → Native Rust Client: Port Plan

Source audited: `legcord-ref/` (github.com/Legcord/Legcord @ `dev`, v1.3.0, OSL-3.0).
Target: a fully native Rust Discord desktop client. No Electron, no Chromium, no injected web app.

---

## 0. Reality check

Legcord is **not** a Discord client. It is a ~14k-LOC Electron shell that loads
`https://discord.com/app` and monkey-patches it with JavaScript. The audit confirms this:

- `src/discord/window.ts:462` — `loadURL("https://discord.com/app")`. Nothing is bundled.
- `src/discord/extensions/modloader.ts` — downloads Vencord/Equicord/Shelter JS bundles.
- `src/discord/preload/patches.mts` (268 lines) — WebRTC SDP munging, camera workarounds.

So "rewriting it in Rust" means **writing a Discord client from scratch**, because there is no
Legcord logic worth porting — the value was all in hosting someone else's app.

What actually gets reused from the audit is the **feature list**, not the code.

### Legal reality

Discord's ToS (§ Software in Discord's Services) prohibits exactly this. Legcord already ships
under a disclaimer saying users are breaking ToS. A native client increases the odds of:

1. Account termination (Discord actively detects non-browser TLS/HTTP2 fingerprints — this is why
   `discord_client_gateway` ships Chrome TLS impersonation).
2. Loss of mod/theme ecosystem entirely.

This is a hobby/research project, not a shippable product. Proceed only accepting that.

---

## 1. Feature parity verdict

17 feature areas from the audit, classified:

| # | Area | Verdict | Notes |
|---|---|---|---|
| A | Window chrome (styles, transparency, mica/acrylic/vibrancy) | **Partial** | egui/winit can't do `windowMaterial`; mica via `windows` crate, vibrancy via `objc2`. Legacy/overlay titlebar modes are meaningless. |
| B | Window state + DPI/multi-display restore | **Full** | `src/common/windowBounds.ts:117-168` is pure geometry logic — ports near-verbatim. |
| C | Tray, unread badges, dock badge, bounce | **Full** | `tray-icon` + `NOTIFY_ICON_TASKBAR` on Windows, `unread_count` on macOS. |
| D | **Vencord / Equicord / Shelter mods** | **LOST** | JS patching a web app. No equivalent exists. **This is the biggest loss.** |
| E | Legcord plugin system (3 targets) | **Replaced** | Reimplement as a native trait-based ABI, or WASM (wasmtime) if you want third-party plugins. ~600 LOC. |
| F | Theming (BetterDiscord themes, Quick CSS, Monaco editor) | **Partial** | egui `Style`/`Theme` gives native theming. BD CSS themes + Quick CSS are **LOST** unless you build a CSS engine. |
| G | Screenshare (source picker, 480p–4K, FPS, loopback, Venmic) | **Hard** | Needs per-platform capture: Win32 `DXGI Desktop Duplication`, X11, Wayland/PipeWire. Loopback audio needs WASAPI loopback / PulseAudio monitor / PipeWire loopback. Venmic equivalent = PipeWire virtual source. |
| H | Performance flags (9 presets, WebRTC stacks, VAAPI) | **Simplified** | `src/common/flags.ts` (476 lines) **deleted entirely** — the single biggest LOC win. No Chromium to tune. |
| I | arRPC / Rich Presence | **Full** | Rust process scanner via `sysinfo`, feed `SetUserActivity` over REST. |
| J | Keybinds, global hotkeys, CLI | **Full** | `global-hotkey` + `clap`. |
| K | Privacy / tracker blocking / CSP / permissions | **Improved** | The 3 blocking rules + localhost-WS allowlist (`src/common/sanitization.ts:37-88`) stay. Everything else (CSP stripping, deny-by-default Electron permissions, WebAuthn stub, Fuses) **deleted** — there is no web content. |
| L | Proxy (5 modes, 3 layers) | **Full** | `reqwest` proxy + PAC. 201 lines → ~80. |
| M | `discord://` deep links | **Full** | `custom-protocol` crate. |
| N | Setup wizard, splash, config migration | **Full** | egui. `src/common/config.ts` (262 lines) + `src/main.ts:231-278` (~20 migrations) ports directly. |
| O | Backup / restore ZIP | **Full** | `zip` crate. |
| P | Platform-specific (TouchBar, VAAPI, D-Bus, mobile) | **Partial** | TouchBar is a deprecated macOS API. Mobile mode was a UA string hack — meaningless here, must be a real responsive layout. |
| Q | Updater, packaging, i18n (33 locales) | **Full** | `self_update` or Tauri CLI bundler; locale JSON ports as-is. |

**Score: 10 full, 5 partial, 1 hard, 2 lost.**

"Retain most features" holds by count. It does *not* hold for the two things Legcord is actually
known for: **client mods and CSS themes**. Decide now whether that's acceptable, because if not,
the honest answer is the Rust-shell + Electron route from the first question.

---

## 2. Crate selection

| Layer | Crate | Why not the alternative |
|---|---|---|
| Gateway | `discord_client_gateway` | **Undetected** client gateway — zlib-stream, Chrome TLS/HTTP2 impersonation, `capabilities: 53607934`, resume. Serenity/Twilight/discordrs are *bot* libraries: intents-gated, no user relationships, no presence-as-client. This is the whole foundation. |
| REST | `discord_client_rest` (same repo) | Matches the gateway's models. |
| Voice / DAVE | `cacophony` + `dave` | `cacophony` = voice gateway, UDP discovery, RTP, AEAD, Opus playout, DAVE coordination. `dave` = libdave codec set (Opus/VP8/VP9/H264/H265/AV1). **Both AGPL-3.0-only** — verify that's acceptable. Fallback: `davey` (OpenMLS, has napi/pyo3 bindings → likely permissive, check LICENSE). |
| Audio capture/mix | `cpal` | Needs `pitch-shifter`/`ringbuf` for 48kHz stereo 20ms frame pacing. |
| Video capture | `nokhwa` or raw `wgpu` Desktop Duplication | nokhwa abstracts Win32/X11 but **not** Wayland. |
| UI | `egui` + `eframe` | GPUI is Windows-first/alpha. iced is weaker on text layout. egui gives immediate-mode settings UI, theming, and a trivial migration for the Solid settings pages. |
| Async | `tokio` | Required by everything above. |
| Hotkeys | `global-hotkey` | XDG portal on Linux, RegisterHotKey on Windows. Replaces `src/dbus.ts` (186 lines). |
| Tray | `tray-icon` | |
| Config | `figment` + `directories` | |
| Proxy | `reqwest` | |
| Zip | `zip` | |
| IPC | `rtrb` / `flume` | Channels from gateway/voice tasks → egui repaint. |
| Plugins | `wasmtime` | Optional: 3rd-party plugin sandbox. |
| Updates | `self_update` | |
| Packaging | Tauri CLI or `cargo-bundle` | Cross-platform installers, MSIX, Flatpak, deb, rpm, dmg. |

---

## 3. Workspace layout

```
tes/
├── Cargo.toml                  # [workspace]
├── crates/
│   ├── dcrs-models/            # gateway + REST payload types, cache normalization
│   ├── dcrs-core/              # Client: gateway loop, REST, cache, event bus
│   ├── dcrs-media/             # voice/video: cacophony + dave + cpal + capture
│   ├── dcrs-rpc/               # process scanner -> presence
│   ├── dcrs-privacy/           # request filter, blocked endpoints
│   ├── dcrs-config/            # settings schema + migrations (port of config.ts)
│   ├── dcrs-ui/                # egui views
│   ├── dcrs-app/               # binary: wiring, tray, window state, CLI
│   └── dcrs-plugins/           # trait ABI + optional wasmtime host
└── assets/                     # locales, icons, tray, badges (reuse from legcord-ref)
```

Architecture rule: `dcrs-core` and `dcrs-media` are **headless and UI-free** — everything
renderable happens in `dcrs-ui`. This keeps them unit-testable without a window.

---

## 4. Phased roadmap

### Phase 0 — Toolchain & skeleton (1–2 days)
- Install Rust (`rustup`, stable + `wasm32-unknown-unknown`).
- Workspace, `rustfmt`/`clippy`, `cargo-nextest`, CI matrix (win/mac/linux).
- Port `assets/lang/*.json` (33 locales) and icons.
- Port `windowBounds.ts` + its tests first (it's pure and already untested in TS — write the tests here).

### Phase 1 — Gateway + REST + login (1–2 weeks)
- `discord_client_gateway` connect, identify, heartbeat, resume, reconnect backoff.
- User login flow. **Hardest unknown**: user auth is not the documented OAuth-bot flow.
  Discord's webapp uses `/api/v9/auth/login` with fingerprinting (WebAuthn stub in
  `preload/patches.mts:9-18` exists precisely because the official webapp disables it).
  Budget real time here.
- Token storage: OS keyring (`keyring` crate), not `localStorage`.
- Cache + event bus.

### Phase 2 — Core UI (3–4 weeks)
- Guild/channel rail, message list (virtualized), composer, member list.
- Direct messages, threads, reactions, edits, replies, markdown + mentions.
- Search, pins, file uploads (CDN resumable), emoji picker.

### Phase 3 — Settings & config (1–2 weeks)
- Port the ~70 settings in `src/@types/settings.d.ts` and `src/common/config.ts`.
- Port all ~20 migrations from `src/main.ts:231-278`.
- egui settings window replacing `SettingsPage.tsx` (1044 lines).
- Quick-CSS / Monaco editor → dropped. Substitute a structured theme editor.

### Phase 4 — Rich presence (1 week)
- Process scanner + custom detectables + blacklist (ports `arpc` fork, `blacklistGame.ts`,
  `detectables.ts`, `RegisteredGamesPage.tsx`, `AddDetectableModal.tsx`).

### Phase 5 — Voice + DAVE (3–4 weeks) ⚠️ **highest risk**
- Voice gateway, UDP discovery, RTP, Opus encode/decode, echo cancel via WebRTC AEC.
- **DAVE E2EE is mandatory now** — Discord requires it for all calls since March 2026.
  OpenMLS group setup + ratchets + protocol transitions. Partially delegated to `cacophony`.
- Mute/deafen/push-to-talk, disconnect.

### Phase 6 — Screen share + video (3–5 weeks) ⚠️ **highest risk**
- Per-platform capture, source picker, resolution/FPS control, loopback audio.
- Replaces the SDP-munging work in `patches.mts:33-135` with direct encoder control.
- Go Live / streaming if you want full parity.

### Phase 7 — Platform polish (2–3 weeks)
- Tray, badges, deep links, window state, updater, installers, backup ZIP, proxy.
- TouchBar (macOS) — deprecated API, low priority.
- Wayland is the long pole on Linux (capture + global hotkeys both need portals).

### Phase 8 — Plugin ABI (1–2 weeks, optional)
- Replace Vencord with a native trait ABI, or wasmtime host for third-party plugins.
- **This is where you could claw back some of feature D.**

---

## 5. Hard blockers

1. **DAVE E2EE.** Mandatory for voice since March 2026. OpenMLS is a heavy, slow-moving
   dependency. If `cacophony`'s DAVE integration is incomplete, you are writing OpenMLS
   yourself. Risk: schedule.
2. **User-mode gateway fingerprinting.** Discord bans client-like TLS/HTTP2 fingerprints.
   `discord_client_gateway` handles this, but it is young (1.2k downloads) and needs
   `client_build_number` kept current — Discord rotates it and stale values get disconnected.
3. **Login/auth.** Not publicly documented. Highest *unknown* risk in Phase 1.
4. **Wayland capture + hotkeys** on Linux.
5. **AGPL-3.0** on `cacophony` + `dave`. If unacceptable, evaluate `davey` or write the
   voice transport yourself.

---

## 6. Effort estimate

Optimistic, single experienced dev, excluding the mod/theme losses:
**~5–6 months** to a usable daily-driver text client. **~9–12 months** with voice, screenshare,
and cross-platform polish. Voice/video alone are ~40% of the total.

---

## 7. Open questions

- Accept losing Vencord/Equicord/Shelter and BetterDiscord themes?
- AGPL-3.0 acceptable, or must the voice stack be permissively licensed?
- Mobile/PinePhone support was a Legcord headline feature — real support, or drop it?
- License for the new project? (Legcord is OSL-3.0; Legcord branding is not reusable.)

---

# Part 2 — Theme & Mod Porting System

Design for recovering as much of Legcord's lost features D and F as possible.

Research basis: source-level survey of `Vendicated/Vencord@718c867`, `BetterDiscord@development`,
`uwu/shelter`, plus empirical selector counts across four real theme corpora (ClearVision v6,
ClearVision v7, the 26-theme `Silverfox0338/discord-themes` corpus, and ClearVision v7's compiled
shipped CSS).

## 8. Correcting the earlier assumption

I previously claimed CSS custom properties were "the leverage point" and that a large slice of
the theme ecosystem would port for free. **The data does not support that.** Measured:

| Corpus | Selector occurrences | Hashed class names | `[class*=…]` prefix | `data-list-id` | Own `var()` defs | Overrides Discord's `--background-primary` |
|---|---|---|---|---|---|---|
| ClearVision v7 (src) | 1,848 | 1,823 (98.7%) | 0 | 0 | 203 | 0 |
| ClearVision v6 | 1,848 | 1,823 (98.7%) | 0 | 0 | — | 0 |
| Silverfox 26-theme corpus | — | 0 | 1,560 | 0 | 483 | 0 |
| **ClearVision v7 (compiled 8.6 KB)** | ~40 | **0** | 0 | 0 | 38 | 0 |

Two things follow:

1. **Themes do not override Discord's variables any more.** `--background-primary` was absent in
   4 of 5 corpora. They define their own (`--main-color`, `--hsl-*`, `--accent-r/g/b`) and consume
   those. Overriding Discord's token set is a *legacy* BetterDiscord pattern.
2. **Maintained themes are ~99% hashed-class selectors**, and they survive hash churn through a
   *generated class map*, not raw strings. ClearVision keeps all 3,198 hashed selectors in one
   file (`src/backend/_classes.scss`, 250 KB) as a Sass map keyed by logical name, so a re-scrape
   repairs all of them at once.

But note row 4: ClearVision's **shipped** artifact is 8.6 KB with zero hashed selectors. The
3,700-entry class map compiles down to essentially `:root`/`.theme-dark` variable overrides plus a
few `:root`-scoped rules, because most hashed selectors resolve to nothing user-visible.

**Revised conclusion:** the porting burden is not in the variables, and it is not in the hashed
selectors either. It is in **translating one naming scheme into another**. That is a mechanical
problem with a mechanical solution, which is what this system is.

## 9. Theme tiers (data-backed)

| Tier | Technique | Corpus weight | Mechanism |
|---|---|---|---|
| **T1** | `:root` / `.theme-dark` / `.theme-light` variable declarations | dominant in shipped CSS | direct token resolution → `ThemeTokens` |
| **T2** | `var(--…)` layering over theme-owned variables | 483 uses (Silverfox) | variable graph resolution, incl. `hsl(calc())` and `--x-hsl` triples |
| **T3** | `[class*="prefix"]` prefix matching | 1,560 uses (Silverfox) | **works natively if we emit prefix-stable class names** |
| **T4** | fully hashed classes (`channel-XXXXX`) | 1,823 uses (ClearVision) | **class-map translation table** (§10) |
| **T5** | structural (`:has()`, `:nth-child()`, `[style^=…]`, `aria-*` state) | ~60 uses | needs a real DOM-ish shadow |
| **T6** | `::before`/`::after` art, `background-image`, custom fonts | varies | asset pipeline |

T3 is nearly free: if `dcrs-ui` names its widgets `channel-*`, `guilds-*`, `messageContent-*`,
`cozyMessage-*`, `expandedFolderBackground-*`, those 1,560 selectors match with no translation.

T4 is the only genuinely hard one, and it is a lookup table, not a parser problem.

## 10. The class-map translation layer — the core idea

ClearVision already solved the maintenance half of this problem in Sass. We do the same in Rust,
as data.

`assets/class-map.json` maps Discord's hashed names to our stable names:

```json
{
  "version": 1,
  "discord_build": "402402",
  "entries": {
    "sidebar_a1":      { "stable": "sidebar",          "surface": "channel-list" },
    "channel_2f1c9d":  { "stable": "channel",          "surface": "channel-list" },
    "chatContent_31":  { "stable": "chat-content",     "surface": "chat" },
    "messageContent_1c07e6": { "stable": "message-content", "surface": "chat" }
  }
}
```

The native UI emits BOTH names on every widget — the hashed name for compatibility, the stable
name for our own use:

```rust
pub struct ClassNames {
    pub stable: &'static str,        // "channel"
    pub hashed: &'static str,        // "channel-2f1c9d" (chosen, hash-stable per build)
}
```

`dcrs-theme` then resolves a theme by:
1. Parsing with `cssparser`.
2. Rewriting every hashed selector through `class-map.json` → stable names.
3. Matching against a **shadow tree** that `dcrs-ui` publishes (see §11).
4. Resolving variables (§12).

Refresh cadence: re-scrape Discord's `:root` and class hashes once per client build, regenerate the
JSON. Same maintenance burden ClearVision already accepts, just in JSON instead of Sass.

## 11. The DOM shadow — how T5 gets supported

`dcrs-ui` maintains a lightweight shadow tree parallel to the egui render pass. Every widget that
has a themeable analogue registers a node:

```rust
pub struct ShadowNode {
    pub tag: &'static str,                       // "div"
    pub role: &'static str,                      // "chat-content"
    pub classes: &'static [&'static str],        // ["channel", "channel-2f1c9d"]
    pub data: &'static [(&'static str, String)],  // [("data-is-self", "true")]
    pub children: Vec<ShadowNode>,
    pub text: Option<String>,
}
```

egui renders from this tree's rects; the shadow exists so the CSS engine has something to select
against. This buys `:has()`, `:nth-child()`, descendant combinators, and attribute selectors —
i.e. all of T5 — for maybe 1,500 LOC, without embedding a browser.

It also lets us **adopt the ecosystem's own escape hatch**. Vencord's `ThemeAttributes` plugin
(`src/plugins/themeAttributes/index.ts`) injects `data-tab-id`, `data-author-id`,
`data-author-username`, `data-is-self`, and `--avatar-url-{128..4096}`. Equicord's `SurfaceClasses`
(`src/api/SurfaceClasses.ts`) lets plugins attach arbitrary `data-*` to eight named surfaces:

```
"base" | "sidebar" | "guildBar" | "channelList" | "membersList" | "titleBar" | "headerBar" | "userArea"
```

Emitting those `data-*` attributes natively is nearly free and makes the small but growing
stable-attribute corpus portable. Note `SurfaceClasses` deliberately *refuses* `className` — the
ecosystem has already decided hashed classes are the weak link.

## 12. `dcrs-theme` — token resolution

```rust
pub struct ThemeTokens { /* ~1206 Discord variable names, grouped */ }
```

Full variable inventory is enumerated in the research: backgrounds/surfaces, background modifiers
and state, text/icons/logo, channels, headers, status/presence, the `visual-refresh` button
families (`--button-outline-*`, `--redesign-button-*`), cards and info boxes, borders/dividers/
shadows/elevation, scrollbars, blur, geometry+typography (`--radius-*`, `--spacing-*`, `--font-*`),
premium/role swatches, third-party brand colors, and the numeric palettes
(`--black-*`, `--white-*`, `--primary-*`, `--neutral-*`, `--brand-*`, `--red-*`, …) each with an
`-hsl` companion form across 32 steps.

Resolution order:
1. Parse `:root`, `.theme-dark`, `.theme-light`, `.theme-darker`, `.theme-midnight`, `.theme-brand`.
2. Build the variable graph; resolve `var()` chains, `hsl(var(--x-hsl) …)` triples, and
   `calc()` lightness offsets — ClearVision's shipped CSS leans on all three.
3. Apply to `ThemeTokens`.
4. Push into egui: colors → `Visuals`, `--radius-*` → `CornerRadius`, `--font-*` → `FontFamily`,
   `--spacing-*` → layout metrics.

**Scope limit to be honest about:** this resolves *values*. It does not lay out boxes. Any theme
rule that changes geometry (`width`, `position`, `transform`, `order`, negative margins) is
Tier-6 and needs per-rule manual work. That is inherent — a native immediate-mode GUI has no CSS
box model.

## 13. `dcrs-compat` — the surface registry

This is the mod side, and it is where the real numbers are.

Measured: **71 Flux stores** (`Vencord.Webpack.Common`, `src/webpack/common/stores.ts`) with
~400 getters; `FluxStore` base with `addChangeListener` / `addConditionalChangeListener` /
`addReactChangeListener` / `syncWith` / `waitFor`; `FluxDispatcher` with `addInterceptor`,
`dispatch({type})`, `subscribe`/`unsubscribe`, `wait`; ~4,000 legal action names
(`packages/discord-types/src/fluxEvents.d.ts`), of which mods actively dispatch ~25.

Each surface is classified:

| Class | Meaning | Examples | Replicable? |
|---|---|---|---|
| **(a) data read** | getters off existing state | 71 stores, `DataStore.get/getMany/keys/values/entries`, `settings.plain`, `BdApi.Data.load`, `Webpack.findCssClasses` | **Yes** — you need a store registry, not 71 reimplementations |
| **(b) data write** | mutations | `DataStore.set/setMany/update/del/delMany/clear`, `SettingsStore.markAsChanged`, `FluxDispatcher.dispatch`, `MediaEngineStore.engine.setLocalVolume/setSelfMute` | **Yes** |
| **(c) subscription** | change notification | `FluxStore.addChangeListener`, `FluxDispatcher.subscribe`, `SettingsStore.addPrefixChangeListener`, `MediaEngineStore.engine.on("DeviceChange"\|"VolumeChange"\|"VideoInputInitialized")`, `webpack.moduleListeners` | **Yes** — one tokio broadcast channel per surface |
| **(d) UI injection** | render a component somewhere | `renderMessageAccessory`, `renderMessageDecoration`, `renderMemberListDecorator`, `chatBarButton`, `messagePopoverButton`, `userProfileBadge`, `settingsAboutComponent`, `toolboxActions`, `contextMenus[navId]`, `ServerList.addServerListElement`, `OptionType.COMPONENT`, 18 `BdApi.Components.*`, `BdApi.UI.*` (11 fns), `Vencord.Api.Notifications`, `Vencord.Api.Notices` | **Yes, free** — we own the render tree |
| **(e) internals patch** | monkeypatch | `patches:[{find,replacement}]`, `Patcher.before/instead/after` (BD) / `spitroast` (shelter), `mapMangledModule`/`getMangled`, `findByCode`/`byStrings`/`byRegex`, `Function.prototype.m` setter hook, `$self` / `#{intl::…}` / `\i` macros | **No** |

**~80% of (a)/(b)/(c)/(d) is a clean contract.** The remaining ~20% is not a contract at all — it
is string-matching against a minified bundle, name-mangling recovery, and source-text replacement.
Every item in (e) breaks on a Discord deploy, and none of it can be reimplemented because it
depends on an artifact the native client does not control.

### Corrections to note from the research

- **Vencord has no `Patcher`.** `Patcher.before/instead/after` is BetterDiscord/Replugged
  semantics; Vencord patches are *declarative* webpack factory source strings
  (`src/webpack/patchWebpack.ts`), one-shot, with no `unpatchAll` — reload is `location.reload()`.
- **`Flux.ChannelStore` does not exist.** Stores live on `Vencord.Webpack.Common.*`, reached via
  `waitForStore` / `filters.byStoreName`.
- **Vencord's `DataStore` has no `observe`/`addKey`/`bulkGet`** and no change notification. It is
  a vendored idb-keyval v6.2.0 over IndexedDB (DB `VencordData`, store `VencordStore`). Reactivity
  comes from `Settings` or flux, never from the KV store. BD's equivalent is synchronous JSON
  (`BdApi.Data.save/load/delete`) with `recache` explicitly discouraged.
- The `Vencord` global has no `load`/`getPlugins`/`reloadPlugin`/`addStyle`/`inject`. It is
  `src/Vencord.ts`'s exports: `Api`, `Plugins` (from `PluginManager`), `Components`, `Util`,
  `Updater`, `Webpack`, `WebpackPatcher`, `Settings`, `PlainSettings`. `startPlugin`/`stopPlugin`
  take a `Plugin` **object**, not a name string.
- `getBadges`, `_modifyAccessories`, `__getDecorators`, `_injectButtons`, `_buildPopoverElements`,
  `_getSurfaceProps` are underscore-prefixed because they are **injected by a patch**. Each is a
  clean (d) contract whose current transport is (e). A native client gets (d) directly.

### The trait ABI

```rust
pub trait Plugin {
    fn manifest(&self) -> &Manifest;
    fn start(&self, cx: &mut PluginCtx) -> Result<()>;
    fn stop(&self, cx: &mut PluginCtx) -> Result<()>;
    fn patches(&self) -> &[Patch] { &[] }
    fn flux(&self) -> &[(FluxEvent, Handler)] { &[] }
    fn render(&self, slot: RenderSlot, ctx: &dyn Any) -> Option<Box<dyn Render>>;
}
```

`Manifest` mirrors `PluginDef` field-for-field so a ported plugin is a recognizable port:
`name`, `description`, `authors`, `searchTerms`, `tags` (the 21 `PluginTag` values),
`commands`, `dependencies`, `required`, `hidden`, `enabledByDefault`, `requiresRestart`,
`startAt` (`Init` | `DOMContentLoaded` | `WebpackReady`), `reporterTestable`, `settings`,
`settingsAboutComponent`, `managedStyle`. `StartAt::WebpackReady` becomes a no-op with a note.

Surface access goes through typed handles, never through a stringly-typed registry at the call
site:

```rust
cx.stores::<ChannelStore>()?.get_channel(id);
cx.stores::<MediaEngineStore>()?.engine.set_self_mute(true);
cx.dispatch(FluxAction::AudioToggleSelfMute);
cx.settings().add_change_listener("myPlugin.foo", cb);
```

## 14. `dcrs-port` — the tool

Input: a `.tsx`/`.ts` Vencord plugin, or a `.css`/`.theme.css` BD theme.

1. **Parse** — `swc`/`oxc` AST walk for JS; `cssparser` for CSS.
2. **Resolve surface usage** — map every `Vencord.Api.*`, `Vencord.Webpack.*`,
   `Vencord.Webpack.Common.*`, `Vencord.Plugins.*`, `BdApi.*`, `Flux.*`, `Patcher.*` reference to a
   `surface::Id`. Classify into (a)–(e).
3. **Classify patch statements** — any `patches:` entry or `Patcher.*` call is (e): report it, do
   not attempt translation, and flag whether the plugin has a non-(e) path.
4. **Cross-reference the capability matrix** (§15).
5. **Emit** — a portability report plus a generated Rust skeleton with the (a)–(d) calls already
   translated and TODOs at every (e) site.

Report shape:

```
plugin: fakeNitro
  (a) reads   ChannelStore.getChannel, UserStore.getCurrentUser            OK
  (b) writes  FluxDispatcher.dispatch(AudioToggleSelfMute)                 OK
  (c) subs    FluxStore.addChangeListener                                  OK
  (d) renders renderMessageAccessory                                        OK
  (e) patches patches:[2 entries]                                          UNSUPPORTED
  settings    6 keys, all OptionType primitive                             OK
  verdict     PARTIAL — 4 of 4 contracts replicable, patch behavior needs native rewrite
```

For CSS:

```
theme: ClearVision-v7
  tier mix   T1 vars 38 | T2 var-refs 38 | T4 hashed 0 | T5 structural 0
  class map  0 entries to translate
  variables  38 declared, 38 resolved, 0 unresolved
  verdict    FULL — ships as T1 only
```

## 15. The capability matrix

One versioned artifact, `assets/capabilities.toml`, recording for every surface:

```toml
[[surface]]
id = "stores.MediaEngineStore"
class = "a,b,c"
impl = "partial"          # Implemented | Partial | Stub | Unsupported
webapp_analogue = "MediaEngineStore"
since = "0.3.0"
notes = "setSelfMute/setLocalVolume done; DeviceChange events pending"
```

Its purpose is to make the loss **measurable and trackable** rather than a feeling. Instead of
"we lost mods," the project can state: *"the themes corpus is 61% T1/T2 (full support), 34% T4
(class-map translation, N of 3,198 entries mapped), 5% T6 (manual)."* And every `impl` flip from
`Stub` to `Implemented` is a visible changelog entry.

## 16. Net effect on the parity verdict

Updating §1's scores:

| Area | Was | Now | Change |
|---|---|---|---|
| **D — client mods** | LOST | **Partial** | (a)/(b)/(c)/(d) ≈ 80% replicable; (e) plugins are not. Realistic target: maybe 30–50% of the popular plugin corpus. |
| **F — theming** | Partial | **Partial (better)** | T1/T2/T3 full, T4 via class map, T5 via shadow tree, T6 manual. |

Cost: +4 crates, and roughly **+4–6 weeks** on top of the §6 estimate, pushed into Phase 8 —
**except** the `dcrs-compat` surface design, which must be settled in Phase 1. Retrofitting an ABI
after the UI and cache exist is genuinely painful, and the shadow-tree naming contract
(`data-*` + `channel-*` prefixes) has to be decided before widgets are written, because retrofitting
stable names across a codebase is worse than retrofitting an ABI.

## 17. Corrections to this document

Two claims made earlier in this file are superseded by §8:
- "Expose Discord's CSS custom properties as the theme contract" — the corpus does not use them
  much. Keep them (they're cheap and Tier 1 themes exist) but they are not the leverage point.
- "Themes would port nearly for free" — wrong. T4 is the bulk of the corpus and needs a
  translation table. The translation is mechanical, not free.

The honest one-line version: **~80% of the mod API surface and ~95% of the shipped theme CSS are
portable; the irreducible loss is webpack internals patching and non-Opus video.**
