# Porting design

How `dcrs` decides what a theme or mod can keep, and why.

## The two losses

A native client has no web app. That kills both of Legcord's headline features:

- **Mods.** Vencord/Equicord/Shelter patch the web app at runtime. There is nothing to patch.
- **Themes.** BetterDiscord themes target Discord's obfuscated CSS class names.

## What the survey actually found

Sources: `Vendicated/Vencord@718c867`, `BetterDiscord@development`, `uwu/shelter`, plus four
theme corpora (ClearVision v6, ClearVision v7, the 26-theme `Silverfox0338/discord-themes` set, and
ClearVision v7's compiled 8.6 KB artifact).

### Mods split into five classes

| Class | Meaning | Examples | Replicable |
|---|---|---|---|
| **a** data read | getters over existing state | 71 Flux stores, ~400 getters; `DataStore.get/keys`; `settings.plain` | yes — needs a store registry, not 71 reimplementations |
| **b** data write | mutations | `DataStore.set/del`; `FluxDispatcher.dispatch`; `MediaEngineStore.engine.setSelfMute` | yes |
| **c** subscription | change notification | `FluxStore.addChangeListener`; `FluxDispatcher.subscribe`; `engine.on("DeviceChange")` | yes — one broadcast channel per surface |
| **d** UI injection | render a component somewhere | `renderMessageAccessory`, `chatBarButton`, `userProfileBadge`, `contextMenus[navId]`, 18 `BdApi.Components.*` | yes, and **free** |
| **e** internals patch | monkeypatch | `patches:[{find,replacement}]`, `Patcher.before/instead/after`, `findByCode`, `getMangled`, `Function.prototype.m` setter hook, `$self` / `#{intl::KEY}` / `\i` macros | **no** |

**~80% of the surface is a clean contract.** The remaining ~20% is not a contract at all — it is
string-matching against a minified bundle, name-mangling recovery, and source-text replacement.
Every item in (e) breaks on a Discord deploy, and none of it can be reimplemented because it
depends on an artifact the client does not control.

Class (d) deserves emphasis: those contracts are only reachable upstream because Vencord patches
Discord's JSX render path (`MessageAccessories._modifyAccessories` exists solely because
`src/plugins/_api/messageAccessories.ts` patches it). The *contract* is clean; the *delivery
mechanism* is (e). A native client that owns its render tree gets (d) directly.

### Corrections to common assumptions

These were believed before the survey and are false:

- **Vencord has no `Patcher`.** `before`/`instead`/`after` are BetterDiscord/Replugged semantics.
  Vencord patches are *declarative* webpack factory source strings (`src/webpack/patchWebpack.ts`),
  one-shot, with no `unpatchAll` — reload is `location.reload()`.
- **`Flux.ChannelStore` does not exist.** Stores are on `Vencord.Webpack.Common.*`, found via
  `waitForStore` / `filters.byStoreName`.
- **Vencord's `DataStore` has no `observe`/`addKey`/`bulkGet` and no change notification.** It is a
  vendored idb-keyval v6.2.0 over IndexedDB. Reactivity comes from `Settings` or flux, never the KV
  store. BetterDiscord's equivalent is synchronous JSON (`BdApi.Data.save/load/delete`) where
  `recache` is explicitly discouraged.
- **The `Vencord` global has no `load`/`getPlugins`/`reloadPlugin`/`addStyle`/`inject`.** It is
  `src/Vencord.ts`'s exports: `Api`, `Plugins` (from `PluginManager`), `Components`, `Util`,
  `Updater`, `Webpack`, `WebpackPatcher`, `Settings`, `PlainSettings`. `startPlugin`/`stopPlugin`
  take a `Plugin` **object**, not a name string.

### Themes are not mostly variables

Measured selector occurrences:

| Corpus | Total | Hashed classes | `[class*=…]` | `data-list-id` | Overrides `--background-primary` |
|---|---|---|---|---|---|
| ClearVision v7 (src) | 1,848 | 1,823 (98.7%) | 0 | 0 | 0 |
| ClearVision v6 | 1,848 | 1,823 (98.7%) | 0 | 0 | 0 |
| Silverfox 26-theme set | — | 0 | 1,560 | 0 | 0 |
| **ClearVision v7 (compiled)** | ~40 | **0** | 0 | 0 | 0 |

Two conclusions:

1. Themes do **not** override Discord's variables any more — they define their own (`--main-color`,
   `--hsl-*`) and consume those. Overriding `--background-primary` is a legacy pattern.
2. Maintained themes survive hash churn through a *generated class map*, not raw strings.
   ClearVision keeps all 3,198 hashed selectors in one file (`src/backend/_classes.scss`, 250 KB)
   as a Sass map keyed by logical name, so a re-scrape repairs all of them at once.

Note the last row: the **shipped** artifact is 8.6 KB with zero hashed selectors. The class map
compiles down to `:root`-scoped variable overrides, because most hashed selectors resolve to nothing
user-visible.

`data-list-id` appears in **zero** of the measured corpora. It is a known Reddit-snippet idiom, not
what shipping themes use.

## The tiers

| Tier | Technique | Corpus weight | Mechanism |
|---|---|---|---|
| T1 | `:root` / `.theme-dark` variable declarations | dominant in shipped CSS | direct token resolution |
| T2 | `var()` layering, `--x-hsl` triples, `calc()` | 483 uses (Silverfox) | variable graph resolution |
| T3 | `[class*="prefix"]` prefix matching | 1,560 uses | **free** if the UI emits `channel-`, `guilds-`, `messageContent-` |
| T4 | fully hashed classes | 1,823 uses | `class-map.json` translation table |
| T5 | `:has()`, `:nth-child()`, `aria-*` | ~60 uses | the shadow tree |
| T6 | geometry, `::before` art, custom fonts | varies | **out of scope** |

T3 is nearly free. T4 is the bulk, and it is a lookup table rather than a parser problem.

## Architecture

```
dcrs-theme
├── classmap.rs   hashed <-> stable name translation, versioned JSON
├── css.rs        tolerant CSS subset parser, var() graph resolution, selector rewriting
├── shadow.rs     element tree for :has() / :nth-child() / attribute selectors
└── theme.rs      manifest scraping, translation, unmapped-class reporting

dcrs-compat       capability registry: surface -> class -> support -> verdict
dcrs-plugins      Plugin trait, Manifest, PluginHost with dependency-ordered lifecycle
dcrs-port         the CLI: analyze a theme or plugin, print a verdict
```

### The class map

ClearVision already solved the maintenance half in Sass. `dcrs-theme` does it in JSON:

```json
{
  "channel-2f1c9d": { "stable": "channel", "surface": "channel-list" }
}
```

Maintenance cadence is identical to ClearVision's: once per Discord client build, re-scrape, add
entries. `dcrs-port theme` reports the unmapped remainder, which is the work queue.

Translation rewrites both `.hashed` selectors and `[class*="hashed"]` / `[class^="hashed"]`
attribute forms, and prefers the longest match so a short name cannot partially rewrite a longer one.

### The shadow tree

`dcrs-ui` publishes a minimal element tree alongside its immediate-mode render pass. Enough
structure for selectors, nothing else — no layout, no box model, no events. That buys T5 for
~1,500 lines without embedding a browser.

It also adopts the ecosystem's own escape hatch. Vencord's `ThemeAttributes`
(`src/plugins/themeAttributes/index.ts`) injects `data-tab-id`, `data-author-id`,
`data-author-username`, `data-is-self`, `--avatar-url-*`. Equicord's `SurfaceClasses`
(`src/api/SurfaceClasses.ts`) lets plugins attach arbitrary `data-*` to eight named surfaces:

```
"base" | "sidebar" | "guildBar" | "channelList" | "membersList" | "titleBar" | "headerBar" | "userArea"
```

`SurfaceClasses` deliberately *refuses* `className`. The ecosystem has already decided hashed
classes are the weak link, and a native client emitting those `data-*` attributes natively is
nearly free.

### Why values, not layout

`dcrs-theme` resolves declarations. It does not lay out boxes. `width`, `height`, `position`,
`transform`, `order`, negative margins and `flex`/`grid` templates cannot be honoured, because an
immediate-mode GUI has no CSS box model to honour them with.

`dcrs-port theme` counts those rules and reports them as `geometry rules`, rather than accepting
them silently. A theme verdict of `MOSTLY FULL` means exactly that: the colours work, the layout
changes do not.

## The capability matrix

`assets/capabilities.toml` records, per surface: `class`, `support`, the webapp analogue, and
notes. Two invariants are enforced in code:

- Only `internals` surfaces may be `unsupported`. A replicable surface that is merely unbuilt is a
  tracking gap, not a ceiling.
- `Registry::coverage()` excludes internals from its denominator, so 100% is reachable.

That is what makes the loss **measurable**. Instead of "we lost mods", the project can state: 19
read surfaces, 6 write, 8 subscription, 14 UI — and 15 internals surfaces that are permanently out
of reach, which is the honest ceiling.

## Maintenance workflow

1. Discord ships a new client build; class hashes change.
2. `dcrs-port theme <theme>` with the previous class map; collect the unmapped list.
3. Scrape the new class list, append entries to `assets/class-map.json`, bump `discord_build`.
4. Re-run; the unmapped count should drop to zero for themes that were previously full.

## What is not solved

- T6 geometry, and `::before`/`::after` used as artwork.
- Class (e) internals patching. Not partially, not at all.
- Video codecs. Discord voice/video moved to mandatory DAVE E2EE in March 2026, which is a separate
  problem from theming (see `PORT-PLAN.md` § 5).
- Third-party plugin distribution. The ABI is here; a sandbox is not.
