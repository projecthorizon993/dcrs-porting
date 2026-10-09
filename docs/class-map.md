# Class map maintenance

`assets/class-map.json` translates Discord's obfuscated CSS class names into the stable names the
native client emits. It is the same idea ClearVision uses with `src/backend/_classes.scss`, except
in JSON.

## Why it exists

Discord ships classes like `channel-2f1c9d`: a human-readable stem plus a hash that changes on every
deploy. Maintained themes target those names — ClearVision v7 has 3,198 of them — so a native client
that emits stable names needs a translation table to keep those themes working.

## Format

```json
{
  "version": 1,
  "discord_build": "402402",
  "entries": {
    "channel-2f1c9d": { "stable": "channel", "surface": "channel-list" }
  }
}
```

- `version` — schema version. The loader rejects anything above `MAX_CLASS_MAP_VERSION` (1).
- `discord_build` — the Discord client build these were scraped from. Informational, so you can tell
  whether a map is stale.
- `stable` — must be a valid CSS class (`[A-Za-z_-][A-Za-z0-9_-]*`). The loader rejects anything
  else, because an invalid stable name silently produces selectors that never match.
- `surface` — one of `guild-bar`, `channel-list`, `chat`, `member-list`, `title-bar`, `header-bar`,
  `user-area`, `overlay`, `settings`, `unknown`. Optional; used to report which regions a theme
  touches.

Two hashed names may map to the same stable name. That is intentional: Discord ships several
variants of a component, and themes often target more than one.

## Update workflow

1. Discord ships a new build and the hashes change.
2. Run the analyzer with the **old** map to get the work queue:

   ```console
   $ dcrs-port --class-map assets/class-map.json theme ~/themes/ClearVision-v7.theme.css
   theme: ClearVision
     class map     0/3198 hashed selectors mapped
       unmapped: channel-1a2b3c, chatContent-4d5e6f, ...
   ```

3. Scrape the new class list from the deployed web client.
4. Append or update entries. Delete entries whose names no longer appear, unless a theme still needs
   them.
5. Set `discord_build` to the build you scraped.
6. Re-run the analyzer; the unmapped count should drop.

## What does not need an entry

The legacy BetterDiscord `[class*="prefix"]` idiom — ~1,560 uses in the Silverfox corpus — matches on
the human-readable prefix rather than the hash. Those selectors work against any client that emits
names like `channel-`, `guilds-`, `messageContent-` or `expandedFolderBackground-`.

That is a requirement on `dcrs-ui`, not on this file: **emit the stable prefix names natively.**
Doing so makes a large slice of the legacy corpus portable with no translation at all.

## Diagnostics

`classmap::looks_hashed` distinguishes the two cases, which is how the analyzer decides whether a
class needs a mapping:

```rust
assert!(classmap::looks_hashed("channel-2f1c9d"));
assert!(!classmap::looks_hashed("channel"));
assert!(!classmap::looks_hashed("chat-content"));
```

A hashed name is a stem followed by a `-` and an alphanumeric suffix of at least 3 characters
starting with a digit.
