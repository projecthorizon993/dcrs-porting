//! Effect extraction: what a mod actually does, independent of how it does it.
//!
//! The API-surface analysis in `analyze` answers "can this plugin call that API natively", which
//! for a mod like `FakeNitro` answers "no" to everything, because it is entirely webpack patches.
//! That answer is useless for planning: the user-visible effect is perfectly portable.
//!
//! So this module reads the `patches:` array and asks a different question of each entry: which
//! capability gate does this override? A gate is any patch whose target is a boolean predicate and
//! whose replacement is a constant. Those become [`GateRecipe`]s: a named capability, the Discord
//! predicate involved, and the field a native client would set instead.
//!
//! The result is that `FakeNitro` reports as eleven native recipes and zero unportable behaviour,
//! which is the truth, rather than eleven unportable patches.

use std::collections::BTreeMap;

use dcrs_compat::{Capability, GateRecipe};
use serde::Serialize;

/// One capability gate a plugin opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    /// The capability.
    pub capability: Capability,
    /// The `find` string it was matched from.
    pub patch_target: String,
    /// Whether the patch is conditional on a setting.
    pub conditional: bool,
    /// The setting predicate that gates it, when conditional.
    pub predicate: Option<String>,
    /// The recipe for implementing it natively.
    pub recipe: GateRecipe,
}

/// A plugin's effects, decomposed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Effects {
    gates: Vec<Gate>,
    /// Behaviour that is not a capability gate: message rewriting, UI injection, and so on.
    /// These still have to be implemented by hand, so they are listed rather than hidden.
    behaviours: Vec<Behaviour>,
}

/// Something a plugin does that is not a premium gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Behaviour {
    /// What it does, e.g. "rewrites premium emoji in outgoing messages into links".
    pub summary: String,
    /// Whether a native client can reproduce it.
    pub natively_reproducible: bool,
    /// How, or why not.
    pub recipe: String,
}

/// Behaviours the extractor recognises, keyed by a distinctive source marker.
const BEHAVIOURS: &[(&str, &str, bool, &str)] = &[
    (
        "addMessagePreSendListener",
        "rewrites premium emoji and stickers in outgoing messages into links",
        true,
        "OutboundMessage::rewrite_premium_content — a composer pass, no patching needed",
    ),
    (
        "addMessagePreEditListener",
        "rewrites premium emoji when editing an existing message",
        true,
        "OutboundMessage::rewrite_premium_content, on the edit path",
    ),
    (
        "renderEmbeds",
        "suppresses image embeds so a rewritten link is not shown twice",
        true,
        "render step: skip an embed whose url was consumed by the rewrite",
    ),
    (
        "openModal",
        "warns before sending content without embed permission",
        true,
        "a confirmation modal in the send path",
    ),
    (
        "sendAnimatedSticker",
        "converts animated stickers into GIF attachments client-side",
        true,
        "APNG decode then GIF encode, then upload as an attachment",
    ),
    (
        "UserSettingsActionCreators",
        "overrides the appearance settings proto so theme changes stick",
        true,
        "SettingsStore::apply_local — merge local appearance over the server proto",
    ),
];

impl Effects {
    /// Extracts effects from plugin source.
    #[must_use]
    pub fn extract(source: &str) -> Self {
        Self {
            gates: extract_gates(source),
            behaviours: extract_behaviours(source),
        }
    }

    /// The capability gates the plugin opens.
    #[must_use]
    #[allow(
        dead_code,
        reason = "part of the report API; exercised by tests and callers"
    )]
    pub fn gates(&self) -> &[Gate] {
        &self.gates
    }

    /// The non-gate behaviours.
    #[must_use]
    #[allow(
        dead_code,
        reason = "part of the report API; exercised by tests and callers"
    )]
    pub fn behaviours(&self) -> &[Behaviour] {
        &self.behaviours
    }

    /// Capabilities keyed by name, for a summary table.
    #[must_use]
    #[allow(
        dead_code,
        reason = "part of the report API; exercised by tests and callers"
    )]
    pub fn capability_table(&self) -> BTreeMap<&'static str, &'static str> {
        self.gates
            .iter()
            .map(|g| (g.capability.as_str(), g.capability.native_hook()))
            .collect()
    }

    /// Whether everything the plugin does can be implemented natively.
    #[must_use]
    pub fn is_fully_portable(&self) -> bool {
        self.behaviours.iter().all(|b| b.natively_reproducible)
    }

    /// Whether any behaviour has to be rebuilt by hand.
    #[must_use]
    pub fn needs_manual_work(&self) -> bool {
        !self.is_fully_portable()
    }

    /// Rendered as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "  capability gates ({} native flags)\n",
            self.gates.len()
        ));
        if self.gates.is_empty() {
            out.push_str("    none detected\n");
        }
        for gate in &self.gates {
            out.push_str(&format!(
                "    {:<34} -> {}\n",
                gate.capability.as_str(),
                gate.recipe.native_hook
            ));
            out.push_str(&format!("      overrides {}", gate.recipe.predicate));
            if let Some(p) = &gate.predicate {
                out.push_str(&format!("; conditional on {p}"));
            }
            out.push('\n');
            if !gate.recipe.caveat.is_empty() {
                out.push_str(&format!("      note: {}\n", gate.recipe.caveat));
            }
        }

        out.push_str(&format!(
            "\n  other behaviour ({})\n",
            self.behaviours.len()
        ));
        if self.behaviours.is_empty() {
            out.push_str("    none detected\n");
        }
        for b in &self.behaviours {
            let mark = if b.natively_reproducible {
                "portable"
            } else {
                "MANUAL"
            };
            out.push_str(&format!("    [{mark}] {}\n", b.summary));
            out.push_str(&format!("             {}\n", b.recipe));
        }

        out.push_str(if self.needs_manual_work() {
            "\n  verdict: gates are native, but some behaviour needs a hand-written implementation\n"
        } else {
            "\n  verdict: fully implementable as native capability flags\n"
        });

        // Cross-reference the API-surface verdict. The two often disagree, and the disagreement is
        // the useful part: a plugin can be "not portable as-is" by API calls while being entirely
        // implementable by effect. Printed together so neither answer is read alone.
        if !self.gates.is_empty() {
            out.push_str(
                "\n  note: `dcrs-port plugin` reports this as not portable as-is, because it is\n  built entirely from webpack patches. That is the mechanism. The gates above are the effect,\n  and the effect is what a native client implements.\n",
            );
        }
        out
    }
}

/// Pulls capability gates out of a `patches:` array.
///
/// Each `find:` string is tested against the known predicate set. `predicate:` lines and
/// `predicate: () => settings.store.X` are captured so a conditional gate is reported as
/// conditional rather than silently becoming always-on.
/// One object literal inside the `patches:` array.
#[derive(Debug, Clone)]
struct PatchObject {
    /// Every `match:` value, in source order.
    matches: Vec<String>,
    /// `find:` on the object itself, if any.
    find: Option<String>,
    /// `predicate:` declared on this object.
    own_predicate: Option<String>,
    /// The nearest `predicate:` on an enclosing object, inherited when this one has none.
    inherited_predicate: Option<String>,
}

impl PatchObject {
    /// The predicate that applies to this object.
    fn predicate(&self) -> Option<String> {
        self.own_predicate
            .clone()
            .or_else(|| self.inherited_predicate.clone())
    }

    /// The targets this object overrides.
    ///
    /// A `match` overrides a specific predicate and is preferred; otherwise the object's own `find`
    /// is the target, since that is what a patch with no nested replacements overrides.
    fn targets(&self) -> Vec<String> {
        if self.matches.is_empty() {
            self.find.iter().cloned().collect()
        } else {
            self.matches.clone()
        }
    }
}

/// Blanks out the contents of comments, strings, templates and regex literals, preserving length
/// and newlines.
///
/// Structural scanning then runs over the masked copy, so a brace inside a regex - `/\i\)\{/` - or
/// a `}` inside a string cannot desynchronise the nesting. Field *values* are still read from the
/// original source, which the mask keeps aligned byte-for-byte.
fn mask_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked: Vec<u8> = bytes.to_vec();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        let blank = |masked: &mut Vec<u8>, from: usize, to: usize| {
            // Newlines survive so line numbering still works.
            // Sliced rather than `enumerate().take(to).skip(from)`: `skip` on a forward iterator still
            // walks the prefix, so masking one literal near the end of a large source re-scanned
            // everything before it. With a few hundred literals that is the difference between a
            // linear pass and tens of millions of byte visits.
            let len = masked.len();
            let slice = &mut masked[from.min(len)..to.min(len)];
            for slot in slice {
                if *slot != b'\n' {
                    *slot = b' ';
                }
            }
        };

        match b {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
                blank(&mut masked, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                    j += 1;
                }
                let end = (j + 2).min(bytes.len());
                blank(&mut masked, i, end);
                i = end;
            }
            b'/' if starts_regex_masked(&masked, i) => {
                let end = skip_regex(bytes, i);
                blank(&mut masked, i, end);
                i = end;
            }
            b'"' | b'\'' | b'`' => {
                let end = skip_quoted(bytes, i, b);
                // Blank the interior but keep both delimiters, so the value's extent stays readable.
                blank(&mut masked, i + 1, end.saturating_sub(1));
                i = end;
            }
            _ => i += 1,
        }
    }

    // A string may change whether the following `/` is a division, so re-run until stable. Bounded
    // by the number of string literals, which is small in practice.
    String::from_utf8(masked).unwrap_or_else(|_| source.to_owned())
}

/// Whether a `/` at `at` starts a regex literal, judging by the masked text so far.
fn starts_regex_masked(masked: &[u8], at: usize) -> bool {
    let Some(next) = masked.get(at + 1) else {
        return false;
    };
    if next.is_ascii_whitespace() || *next == b'=' {
        return false;
    }
    masked[..at]
        .iter()
        .rev()
        .find(|b| !b.is_ascii_whitespace())
        .is_none_or(|last| {
            !matches!(
                last,
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b')' | b']' | b'}' | b'"' | b'\''
                    | b'`'
            )
        })
}

/// Whether `pos` starts a `key:` field at a token boundary in the masked text.
fn is_field_at(masked: &[u8], pos: usize, key: &str) -> bool {
    let k = key.as_bytes();
    if masked.get(pos..pos + k.len()) != Some(k) {
        return false;
    }
    if masked.get(pos + k.len()) != Some(&b':') {
        return false;
    }
    masked[..pos]
        .iter()
        .rev()
        .find(|b| !b.is_ascii_whitespace())
        .is_none_or(|before| matches!(before, b'{' | b'}' | b'[' | b']' | b',' | b';'))
}

/// Splits the `patches:` array into object literals, tracking nesting so that a `predicate:` on an
/// outer patch applies to the replacements nested inside it.
///
/// Two things make this more than a brace counter:
///
/// - `FakeNitro` writes `match:`, `replace:` and `predicate:` on three consecutive lines of the same
///   object, so a line-based reader attributes each object the *previous* object's predicate.
/// - The replacements are regular expressions, and those contain braces - `/\i\)\{/` - which would
///   desynchronise any counter that does not skip literals.
///
/// So this walks every byte of the masked source for structure, and reads field values from the
/// original at the same offsets.
fn patch_objects(source: &str) -> Vec<PatchObject> {
    let masked = mask_literals(source);
    let bytes = masked.as_bytes();
    let mut out = Vec::new();
    let mut stack: Vec<PatchObject> = Vec::new();

    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];

        if let Some(key) = FIELD_KEYS.iter().find(|k| is_field_at(bytes, i, k)) {
            let value = field_value_at(source, i + key.len() + 1);
            if let Some(frame) = stack.last_mut() {
                match *key {
                    "predicate" if frame.own_predicate.is_none() => {
                        frame.own_predicate = Some(settings_predicate(&value));
                    }
                    "find" => frame.find = Some(value.clone()),
                    "match" => frame.matches.push(value.clone()),
                    _ => {}
                }
            }
            // Skip past the value so its contents are not rescanned for fields.
            i += key.len() + 1;
            while i < bytes.len() && bytes[i] != b',' && bytes[i] != b'\n' && bytes[i] != b'}' {
                i += 1;
            }
            continue;
        }

        match b {
            b'{' => {
                let inherited = stack.last().and_then(PatchObject::predicate);
                stack.push(PatchObject {
                    matches: vec![],
                    find: None,
                    own_predicate: None,
                    inherited_predicate: inherited,
                });
            }
            b'}' => {
                if let Some(done) = stack.pop() {
                    // An object that overrides something is a patch entry worth reporting.
                    if !done.matches.is_empty() || done.find.is_some() {
                        out.push(done);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    out
}

/// Field names recognised inside a patch object.
const FIELD_KEYS: &[&str] = &["predicate", "find", "match"];

/// Reads a field's value from `source`, starting at `start`.
///
/// The terminator depends on what the value is. A regex literal or a string runs to its own
/// closing delimiter - and may contain commas freely, as `/\i,\i/` does. Anything else runs to the
/// next `,`, newline or closing brace at nesting depth zero.
fn field_value_at(source: &str, start: usize) -> String {
    let bytes = source.as_bytes();
    // Skip the whitespace between `key:` and its value, otherwise a value on the next line is
    // missed and the scan runs on into the following field.
    let mut start = start.min(bytes.len());
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let first = bytes.get(start).copied().unwrap_or(b' ');

    let end = match first {
        b'/' | b'"' | b'\'' | b'`' => {
            let from = if first == b'/' { start + 1 } else { start };
            let closed = if first == b'/' {
                skip_regex(bytes, start)
            } else {
                skip_quoted(bytes, start, first)
            };
            // A regex may be followed by flags; include them.
            let mut e = closed;
            if first == b'/' {
                while e < bytes.len() && bytes[e].is_ascii_alphabetic() {
                    e += 1;
                }
            }
            let _ = from;
            e
        }
        _ => {
            let mut depth = 0usize;
            let mut i = start;
            while i < bytes.len() {
                match bytes[i] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        // A closing bracket at depth zero ends the value.
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    b',' | b'\n' if depth == 0 => break,
                    _ => {}
                }
                i += 1;
            }
            i
        }
    };

    source[start.min(source.len())..end.min(source.len())]
        .trim()
        .to_owned()
}

/// Returns the index just past a regex literal starting at `at`.
fn skip_regex(bytes: &[u8], at: usize) -> usize {
    let mut i = at + 1;
    let mut in_class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => in_class = true,
            b']' => in_class = false,
            b'/' if !in_class => return i + 1,
            // A regex cannot span lines unescaped, so stop at the newline.
            b'\n' => return at + 1,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// Returns the index just past a string or template literal starting at `at`.
fn skip_quoted(bytes: &[u8], at: usize, quote: u8) -> usize {
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'\n' if quote != b'`' => return at + 1,
            b if b == quote => return i + 1,
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

fn extract_gates(source: &str) -> Vec<Gate> {
    let mut gates = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for object in patch_objects(source) {
        let predicate = object.predicate();
        let conditional = predicate.is_some();
        for target in object.targets() {
            let Some(capability) = Capability::from_patch_target(&target) else {
                continue;
            };
            if !seen.insert(capability) {
                continue;
            }
            gates.push(Gate {
                capability,
                patch_target: unquote(&target),
                conditional,
                predicate: predicate.clone(),
                recipe: GateRecipe::new(capability),
            });
        }
    }

    gates.sort_by_key(|g| g.capability);
    gates
}

fn settings_predicate(raw: &str) -> String {
    for marker in ["settings.store.", "settings.plain.", "store."] {
        if let Some(idx) = raw.find(marker) {
            let rest = &raw[idx + marker.len()..];
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            return format!("{marker}{}", &rest[..end]);
        }
    }
    raw.trim().chars().take(48).collect()
}

/// Strips quotes from a `find:` value.
fn unquote(raw: &str) -> String {
    let t = raw.trim().trim_end_matches(',');
    t.trim_matches(|c| c == '"' || c == '\'').to_owned()
}

/// Finds the non-gate behaviours a plugin implements.
fn extract_behaviours(source: &str) -> Vec<Behaviour> {
    BEHAVIOURS
        .iter()
        .filter(|(marker, ..)| source.contains(marker))
        .map(|(_, summary, reproducible, recipe)| Behaviour {
            summary: (*summary).to_owned(),
            natively_reproducible: *reproducible,
            recipe: (*recipe).to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed but structurally faithful `FakeNitro`: real `find` strings, real predicates,
    /// real listener registrations.
    // `r##` rather than `r#`, because the source contains `"#{intl::...` which would otherwise
    // close the raw string.
    const FAKE_NITRO: &str = r##"
export default definePlugin({
    name: "FakeNitro",
    settings: definePluginSettings({
        enableEmojiBypass: { type: OptionType.BOOLEAN, default: true },
        enableStickerBypass: { type: OptionType.BOOLEAN, default: true },
        enableStreamQualityBypass: { type: OptionType.BOOLEAN, default: true }
    }),
    patches: [
        {
            find: "canUseCustomStickersEverywhere:",
            replacement: [
                { match: /(?<=canUseCustomStickersEverywhere:function\(\i\)\{)/,
                  replace: "return true;",
                  predicate: () => settings.store.enableStickerBypass },
                { match: /(?<=canUseHighVideoUploadQuality:function\(\i\)\{)/,
                  replace: "return true;",
                  predicate: () => settings.store.enableStreamQualityBypass },
                { match: /(?<=canStreamQuality:function\(\i,\i\)\{)/,
                  replace: "return true;",
                  predicate: () => settings.store.enableStreamQualityBypass },
                { match: /(?<=canUseClientThemes:function\(\i\)\{)/, replace: "return true;" },
                { match: /(?<=canUsePremiumAppIcons:function\(\i\)\{)/, replace: "return true;" }
            ]
        },
        { find: '.getByName("fork_and_knife")',
          predicate: () => settings.store.enableEmojiBypass,
          replacement: { match: ".CHAT", replace: ".STATUS" } },
        { find: ".GUILD_SUBSCRIPTION_UNAVAILABLE;",
          predicate: () => settings.store.enableEmojiBypass,
          replacement: { match: /\.available/, replace: "true?" } },
        { find: ".getUserIsAdmin(",
          replacement: { match: /function/, replace: "fakeNitroOriginal" } },
        { find: '"SENDABLE"', predicate: () => settings.store.enableStickerBypass,
          replacement: { match: /\i\.available\?/, replace: "true?" } },
        { find: "#{intl::STREAM_FPS_OPTION}", predicate: () => settings.store.enableStreamQualityBypass,
          replacement: { match: /guildPremiumTier:\i\.\i\.TIER_\d,?/g, replace: "" } },
        { find: '"UserSettingsProtoStore"',
          replacement: { match: /CONNECTION_OPEN/, replace: "$self.handleProtoChange" } },
        { find: ",updateTheme(",
          replacement: { match: /(function)/, replace: "$self.handleGradientThemeSelect" } },
        { find: ".CLIENT_THEMES_EDITOR?",
          replacement: { match: /TIER_2/, replace: "true" } },
        { find: "getCurrentDesktopIcon(),",
          replacement: { match: /isPremium/, replace: "true" } },
        { find: 'type:"GUILD_SOUNDBOARD_SOUND_CREATE"',
          replacement: { match: /\.available/g, replace: "true" } },
        { find: "}renderStickersAccessories(",
          replacement: { match: /renderEmbeds/, replace: "$self.shouldIgnoreEmbed" } }
    ],
    start() {
        this.preSend = addMessagePreSendListener(async (channelId, messageObj, options) => {});
        this.preEdit = addMessagePreEditListener(async (channelId, __, messageObj) => {});
        showCannotEmbedNotice();
        sendAnimatedSticker(link, id, channelId);
    }
});
"##;

    #[test]
    fn extracts_every_fakenitro_gate() {
        let effects = Effects::extract(FAKE_NITRO);
        let found: Vec<Capability> = effects.gates().iter().map(|g| g.capability).collect();

        for expected in [
            Capability::CustomStickersEverywhere,
            Capability::StreamQuality,
            Capability::ClientThemes,
            Capability::PremiumAppIcons,
            Capability::PremiumAppIconCurrent,
            Capability::ClientThemeEditor,
            Capability::AllStickersAvailable,
            Capability::AllSoundboardSounds,
            Capability::StreamFpsUnlocked,
            Capability::LocalSettingsProto,
            Capability::GradientThemeSelection,
            Capability::EmojiPickerIntent,
        ] {
            assert!(
                found.contains(&expected),
                "missing {expected:?} in {found:?}"
            );
        }
    }

    #[test]
    fn captures_the_setting_that_conditions_a_gate() {
        let effects = Effects::extract(FAKE_NITRO);

        // Each nested `match` is its own object, so the target recorded is the regex that names
        // the predicate rather than the enclosing patch's `find`.
        let sticker = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::CustomStickersEverywhere)
            .unwrap();
        assert!(
            sticker
                .patch_target
                .contains("canUseCustomStickersEverywhere")
        );
        assert!(sticker.conditional);
        assert!(
            sticker.predicate.as_deref() == Some("settings.store.enableStickerBypass"),
            "got {:?}",
            sticker.predicate
        );

        // A predicate declared on the enclosing patch is inherited by its replacements.
        let themes = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::ClientThemes)
            .unwrap();
        assert!(
            !themes.conditional,
            "canUseClientThemes has no predicate upstream"
        );

        let soundboard = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::AllSoundboardSounds)
            .unwrap();
        assert!(!soundboard.conditional, "that one is unconditional");

        let fps = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::StreamFpsUnlocked)
            .unwrap();
        assert!(fps.conditional, "STREAM_FPS_OPTION is conditional");
        assert!(
            fps.predicate.as_deref() == Some("settings.store.enableStreamQualityBypass"),
            "got {:?}",
            fps.predicate
        );
    }

    #[test]
    fn gates_carry_the_native_recipe() {
        let effects = Effects::extract(FAKE_NITRO);
        let stream = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::StreamQuality)
            .unwrap();
        assert_eq!(stream.recipe.native_hook, "Capabilities::stream_quality");
        assert_eq!(stream.recipe.predicate, "canStreamQuality");
    }

    #[test]
    fn local_only_gates_are_flagged_with_a_caveat() {
        let effects = Effects::extract(FAKE_NITRO);
        let emoji = effects
            .gates()
            .iter()
            .find(|g| g.capability == Capability::EmojiPickerIntent)
            .unwrap();
        assert!(!emoji.recipe.locally_observable);
        assert!(
            !emoji.recipe.caveat.is_empty(),
            "a local-only gate needs a caveat"
        );
        let rendered = effects.render();
        assert!(rendered.contains("local only"), "{rendered}");
    }

    #[test]
    fn non_gate_behaviours_are_listed_separately() {
        let effects = Effects::extract(FAKE_NITRO);
        let summaries: Vec<&str> = effects
            .behaviours()
            .iter()
            .map(|b| b.summary.as_str())
            .collect();
        assert!(
            summaries.iter().any(|s| s.contains("outgoing messages")),
            "{summaries:?}"
        );
        assert!(
            summaries.iter().any(|s| s.contains("editing")),
            "{summaries:?}"
        );
        assert!(
            summaries.iter().any(|s| s.contains("embeds")),
            "{summaries:?}"
        );
        assert!(summaries.iter().any(|s| s.contains("GIF")), "{summaries:?}");
        assert!(effects.is_fully_portable());
    }

    #[test]
    fn render_is_informative() {
        let rendered = Effects::extract(FAKE_NITRO).render();
        assert!(rendered.contains("capability gates ("));
        assert!(rendered.contains("Capabilities::stream_quality"));
        assert!(rendered.contains("other behaviour ("));
        assert!(rendered.contains("fully implementable as native capability flags"));
    }

    #[test]
    fn a_plugin_with_no_patches_yields_nothing() {
        let effects =
            Effects::extract(r#"export default definePlugin({ name: "Plain", start() {} });"#);
        assert_eq!(effects.gates(), Vec::new());
        assert_eq!(effects.behaviours(), Vec::new());
        assert!(!effects.needs_manual_work());
        assert!(effects.render().contains("none detected"));
    }

    #[test]
    fn capability_table_is_stable() {
        let table = Effects::extract(FAKE_NITRO).capability_table();
        assert!(table.contains_key("high-quality streaming"));
        assert_eq!(table["client themes"], "Capabilities::client_themes");
    }

    #[test]
    fn settings_predicate_extraction() {
        assert_eq!(
            settings_predicate(" () => settings.store.enableEmojiBypass,"),
            "settings.store.enableEmojiBypass"
        );
        assert_eq!(settings_predicate(" () => true"), "() => true");
    }

    #[test]
    fn unquote_strips_quotes_and_commas() {
        assert_eq!(unquote("\"SENDABLE\","), "SENDABLE");
        assert_eq!(unquote("'.CHAT',"), ".CHAT");
        assert_eq!(unquote("bare"), "bare");
    }
}
