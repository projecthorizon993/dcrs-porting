//! Static analysis of mod sources: which surfaces a plugin touches, and whether those can be
//! replicated natively.
//!
//! This is deliberately a lexical scanner rather than a full JS parser. Plugin sources are
//! TypeScript, and the goal is inventory, not transformation: we need to know that a plugin reads
//! `Flux.ChannelStore.getChannel` and that it declares a `patches:` entry, not to re-emit the AST.

use std::collections::BTreeMap;
use std::fmt;

use dcrs_compat::{Class, Registry, Verdict};
use serde::Serialize;

/// One surface a plugin depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Usage {
    /// Registry surface id.
    pub surface: String,
    /// How the plugin uses it.
    pub class: Class,
    /// Verdict from the capability registry.
    pub verdict: Verdict,
    /// 1-based line number where the usage was first seen.
    pub line: usize,
    /// The source text that triggered the detection.
    pub snippet: String,
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  ({}) {:<34} {:<16} line {}",
            self.class.as_str(),
            self.surface,
            self.verdict.as_str(),
            self.line
        )
    }
}

/// Patterns that map source constructs to registry surface ids.
struct Detector {
    /// `(needle, surface id, class)`
    needles: &'static [(&'static str, &'static str, Class)],
}

/// Vencord / `BetterDiscord` API constructs, mapped onto registry ids.
///
/// Ids follow the naming in the bundled `assets/capabilities.toml`.
const DETECTORS: &[Detector] = &[
    Detector {
        needles: &[
            (
                "Patcher.before",
                "internals.patcherBefore",
                Class::Internals,
            ),
            (
                "Patcher.instead",
                "internals.patcherInstead",
                Class::Internals,
            ),
            ("Patcher.after", "internals.patcherAfter", Class::Internals),
            (
                "unpatchAll",
                "internals.patcherUnpatchAll",
                Class::Internals,
            ),
            (
                "mapMangledModule",
                "internals.mapMangledModule",
                Class::Internals,
            ),
            ("getMangled", "internals.getMangled", Class::Internals),
            ("findByCode", "internals.findByCode", Class::Internals),
            ("byStrings", "internals.findByStrings", Class::Internals),
            ("byRegex", "internals.findByRegex", Class::Internals),
            ("bySource", "internals.findBySource", Class::Internals),
            ("wreq", "internals.webpackRequire", Class::Internals),
            ("getModule", "internals.getModule", Class::Internals),
        ],
    },
    Detector {
        needles: &[
            ("FluxDispatcher.dispatch", "flux.dispatch", Class::Write),
            (
                "FluxDispatcher.subscribe",
                "flux.subscribe",
                Class::Subscription,
            ),
            (
                "FluxDispatcher.addInterceptor",
                "flux.addInterceptor",
                Class::Subscription,
            ),
            (
                "addChangeListener",
                "stores.addChangeListener",
                Class::Subscription,
            ),
            (
                "addConditionalChangeListener",
                "stores.addConditionalChangeListener",
                Class::Subscription,
            ),
            (
                "removeChangeListener",
                "stores.removeChangeListener",
                Class::Subscription,
            ),
        ],
    },
    Detector {
        needles: &[
            ("ChannelStore", "stores.ChannelStore", Class::Read),
            ("GuildStore", "stores.GuildStore", Class::Read),
            ("UserStore", "stores.UserStore", Class::Read),
            ("MemberStore", "stores.GuildMemberStore", Class::Read),
            ("MessageStore", "stores.MessageStore", Class::Read),
            (
                "SelectedChannelStore",
                "stores.SelectedChannelStore",
                Class::Read,
            ),
            (
                "SelectedGuildStore",
                "stores.SelectedGuildStore",
                Class::Read,
            ),
            ("MediaEngineStore", "stores.MediaEngineStore", Class::Read),
            ("PermissionStore", "stores.PermissionStore", Class::Read),
            ("RelationshipStore", "stores.RelationshipStore", Class::Read),
            ("ReadStateStore", "stores.ReadStateStore", Class::Read),
            ("VoiceStateStore", "stores.VoiceStateStore", Class::Read),
            ("RoleStore", "stores.GuildRoleStore", Class::Read),
            ("PresenceStore", "stores.PresenceStore", Class::Read),
            ("DraftStore", "stores.DraftStore", Class::Read),
            ("EmojiStore", "stores.EmojiStore", Class::Read),
            (
                "UserSettingsProtoStore",
                "stores.UserSettingsProtoStore",
                Class::Read,
            ),
        ],
    },
    Detector {
        needles: &[
            ("DataStore.get", "dataStore.get", Class::Read),
            ("DataStore.set", "dataStore.set", Class::Write),
            ("DataStore.del", "dataStore.del", Class::Write),
            ("DataStore.keys", "dataStore.keys", Class::Read),
            ("BdApi.Data.save", "dataStore.set", Class::Write),
            ("BdApi.Data.load", "dataStore.get", Class::Read),
        ],
    },
    Detector {
        needles: &[
            (
                "renderMessageAccessory",
                "ui.renderMessageAccessory",
                Class::Ui,
            ),
            (
                "renderMessageDecoration",
                "ui.renderMessageDecoration",
                Class::Ui,
            ),
            (
                "renderMemberListDecorator",
                "ui.renderMemberListDecorator",
                Class::Ui,
            ),
            ("chatBarButton", "ui.chatBarButton", Class::Ui),
            ("messagePopoverButton", "ui.messagePopoverButton", Class::Ui),
            ("userProfileBadge", "ui.userProfileBadge", Class::Ui),
            ("settingsAboutComponent", "ui.settingsAbout", Class::Ui),
            ("toolboxActions", "ui.toolbox", Class::Ui),
            ("contextMenus", "ui.contextMenu", Class::Ui),
            ("addServerListElement", "ui.serverListElement", Class::Ui),
        ],
    },
];

/// A plugin's analyzed shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginReport {
    /// Plugin name, from `name:` in the source if present.
    pub name: Option<String>,
    /// Every detected surface usage, sorted.
    pub usages: Vec<Usage>,
    /// Whether a declarative `patches:` array was found.
    pub has_patches: bool,
    /// How many entries the `patches:` array appeared to contain.
    pub patch_count: usize,
    /// Whether `definePlugin` was seen.
    pub is_vendor_plugin: bool,
}

impl PluginReport {
    /// Usages grouped by class, for the summary table.
    #[must_use]
    pub fn by_class(&self) -> BTreeMap<Class, Vec<&Usage>> {
        let mut out: BTreeMap<Class, Vec<&Usage>> = BTreeMap::new();
        for u in &self.usages {
            out.entry(u.class).or_default().push(u);
        }
        out
    }

    /// Whether the plugin has a usage that can never work as-is.
    ///
    /// A surface that is simply not implemented yet is a tracking gap, not a blocker: the client
    /// can still gain support for it later. An internals patch, or a surface explicitly declared
    /// unsupported, is a genuine ceiling.
    #[must_use]
    pub fn has_blocking_issue(&self) -> bool {
        self.has_patches
            || self.usages.iter().any(|u| u.class == Class::Internals)
            || self
                .usages
                .iter()
                .any(|u| u.verdict == Verdict::Unsupported || u.verdict == Verdict::NotReplicable)
    }

    /// Overall verdict line.
    #[must_use]
    pub fn verdict(&self) -> &'static str {
        if self.usages.is_empty() && !self.has_patches {
            return "INCONCLUSIVE - no recognisable API usage found";
        }
        let internals = self
            .usages
            .iter()
            .filter(|u| u.class == Class::Internals)
            .count();
        let unavailable = self
            .usages
            .iter()
            .filter(|u| u.class != Class::Internals && !u.verdict.is_usable())
            .count();

        if self.has_patches {
            "NOT PORTABLE AS-IS - declarative patches target the minified bundle"
        } else if internals == 0 && unavailable == 0 {
            "PORTABLE"
        } else if internals == 0 {
            "PARTIAL - some surfaces not implemented yet"
        } else if unavailable == 0 {
            "PARTIAL - needs native rewrite of internals-dependent behavior"
        } else {
            "NOT PORTABLE AS-IS"
        }
    }

    /// Rendered as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(name) = &self.name {
            out.push_str(&format!("plugin: {name}\n"));
        } else {
            out.push_str("plugin: <unnamed>\n");
        }
        out.push_str(&format!(
            "  definePlugin detected: {}\n",
            self.is_vendor_plugin
        ));

        let grouped = self.by_class();
        for class in [
            Class::Read,
            Class::Write,
            Class::Subscription,
            Class::Ui,
            Class::Internals,
        ] {
            let Some(usages) = grouped.get(&class) else {
                continue;
            };
            out.push_str(&format!(
                "  ({}) {}  [{}]\n",
                class.as_str(),
                class.label(),
                usages.len()
            ));
            for u in usages {
                out.push_str(&format!("{u}\n"));
            }
        }

        if self.has_patches {
            out.push_str(&format!(
                "  (e) patches:[] array with ~{} entries          NOT REPLICABLE\n",
                self.patch_count
            ));
        }
        if self.has_blocking_issue() {
            out.push_str("  blocker: yes - needs a native rewrite or cannot be ported\n");
        }
        out.push_str(&format!("  verdict: {}\n", self.verdict()));
        out
    }
}

/// Errors from analysis.
#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    /// The file could not be read.
    #[error("reading source: {0}")]
    Io(#[from] std::io::Error),
}

/// Analyzes a plugin source file.
///
/// # Errors
/// Currently infallible. [`AnalyzeError`] is reserved so that a real parser can fail later
/// without changing every call site.
#[allow(clippy::unnecessary_wraps)]
pub fn analyze(source: &str, registry: &Registry) -> Result<PluginReport, AnalyzeError> {
    let is_vendor_plugin = source.contains("definePlugin");

    let name = extract_string_field(source, "name");
    let (has_patches, patch_count) = detect_patches(source);

    let mut seen: BTreeMap<String, Usage> = BTreeMap::new();
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
            continue;
        }
        for detector in DETECTORS {
            for (needle, surface_id, class) in detector.needles {
                if !line.contains(needle) {
                    continue;
                }
                let key = format!("{surface_id}@{needle}");
                if seen.contains_key(&key) {
                    continue;
                }
                seen.insert(
                    key,
                    Usage {
                        surface: (*surface_id).to_owned(),
                        class: *class,
                        verdict: registry.verdict(surface_id),
                        line: line_no + 1,
                        snippet: trimmed.chars().take(96).collect(),
                    },
                );
            }
        }
    }

    let mut usages: Vec<Usage> = seen.into_values().collect();
    usages.sort_by(|a, b| a.class.cmp(&b.class).then(a.surface.cmp(&b.surface)));

    Ok(PluginReport {
        name,
        usages,
        has_patches,
        patch_count,
        is_vendor_plugin,
    })
}

/// Finds a `key: "value"` field and returns the string literal.
fn extract_string_field(source: &str, field: &str) -> Option<String> {
    let needle = format!("{field}:");
    let start = source.find(&needle)? + needle.len();
    let rest = source[start..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = &rest[quote.len_utf8()..];
    let end = inner.find(quote)?;
    Some(inner[..end].to_owned())
}

/// Detects a declarative `patches:` array and roughly counts its entries.
///
/// A `patches:` key is matched on its own so it is not confused with the runtime patcher methods,
/// which are reported separately as internals usages.
fn detect_patches(source: &str) -> (bool, usize) {
    let Some(idx) = source.find("patches:") else {
        return (false, 0);
    };
    let rest = &source[idx + "patches:".len()..];
    let Some(open) = rest.find('[') else {
        return (true, 0);
    };
    // Walk to the matching close bracket, respecting nesting and strings.
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' | b'`' => quote = Some(b),
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let body = &rest[open..i.min(rest.len())];
    // Each entry declares a `find:` and a `replacement:`.
    let count = body
        .matches("find:")
        .count()
        .max(body.matches("replacement:").count());
    (true, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcrs_compat::{Registry, Support};

    fn registry() -> Registry {
        Registry::from_surfaces(vec![
            dcrs_compat::Surface {
                id: "stores.ChannelStore".into(),
                class: Class::Read,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "stores.MediaEngineStore".into(),
                class: Class::Read,
                support: Support::Partial,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "stores.VoiceStateStore".into(),
                class: Class::Read,
                support: Support::Stub,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "flux.dispatch".into(),
                class: Class::Write,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "ui.renderMessageAccessory".into(),
                class: Class::Ui,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "internals.patcherBefore".into(),
                class: Class::Internals,
                support: Support::Unsupported,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "internals.findByCode".into(),
                class: Class::Internals,
                support: Support::Unsupported,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
        ])
        .unwrap()
    }

    const CLEAN_PLUGIN: &str = r#"
export default definePlugin({
    name: "FakeNitro",
    description: "nope",
    authors: [{ name: "x", id: BigInt(1) }],
    start() {
        const ch = Vencord.Webpack.Common.ChannelStore.getChannel(1n);
        FluxDispatcher.dispatch({ type: "AUDIO_TOGGLE_SELF_MUTE" });
    },
    renderMessageAccessory() { return null; }
});
"#;

    const PATCHED_PLUGIN: &str = r#"
export default definePlugin({
    name: "NoTrack",
    patches: [
        { find: "analytics:void 0", replacement: "analytics:noop", all: true },
        { find: "sentry.init", replacement: "sentry.init" }
    ],
    start() {
        Patcher.before(SomeModule.prototype, "method", function () {});
    }
});
"#;

    #[test]
    fn detects_reads_writes_and_ui() {
        let r = analyze(CLEAN_PLUGIN, &registry()).unwrap();
        assert_eq!(r.name.as_deref(), Some("FakeNitro"));
        assert!(r.is_vendor_plugin);
        assert!(!r.has_patches);

        let classes: Vec<Class> = r.usages.iter().map(|u| u.class).collect();
        assert!(classes.contains(&Class::Read));
        assert!(classes.contains(&Class::Write));
        assert!(classes.contains(&Class::Ui));
        assert!(!classes.contains(&Class::Internals));
    }

    #[test]
    fn clean_plugin_is_portable() {
        let r = analyze(CLEAN_PLUGIN, &registry()).unwrap();
        assert_eq!(r.verdict(), "PORTABLE");
        assert!(!r.has_blocking_issue());
        assert!(r.render().contains("verdict: PORTABLE"));
    }

    #[test]
    fn detects_declarative_patches_and_counts_them() {
        let r = analyze(PATCHED_PLUGIN, &registry()).unwrap();
        assert!(r.has_patches);
        assert_eq!(r.patch_count, 2);
        assert_eq!(
            r.verdict(),
            "NOT PORTABLE AS-IS - declarative patches target the minified bundle"
        );
        assert!(r.has_blocking_issue());
    }

    #[test]
    fn detects_runtime_patcher_calls() {
        let r = analyze(PATCHED_PLUGIN, &registry()).unwrap();
        let internals: Vec<&Usage> = r
            .usages
            .iter()
            .filter(|u| u.class == Class::Internals)
            .collect();
        assert!(
            internals
                .iter()
                .any(|u| u.surface == "internals.patcherBefore"),
            "expected Patcher.before, got {internals:?}"
        );
        // An internals surface must never report as usable.
        for u in &internals {
            assert!(!u.verdict.is_usable());
        }
    }

    #[test]
    fn reports_line_numbers() {
        let r = analyze(CLEAN_PLUGIN, &registry()).unwrap();
        let usage = r
            .usages
            .iter()
            .find(|u| u.surface == "stores.ChannelStore")
            .unwrap();
        assert!(
            usage.line > 1,
            "usage should point at the actual line, got {}",
            usage.line
        );
        assert!(usage.snippet.contains("ChannelStore"));
    }

    #[test]
    fn skips_commented_out_lines() {
        let src = "// const x = Vencord.Webpack.Common.GuildStore.getGuilds();\nconst y = 1;";
        let r = analyze(src, &registry()).unwrap();
        assert!(
            r.usages.is_empty(),
            "comment should not register: {:?}",
            r.usages
        );
    }

    #[test]
    fn partial_surfaces_surface_in_verdict() {
        let src = r#"
export default definePlugin({
    name: "Loud",
    start() { const e = MediaEngineStore.engine; }
});
"#;
        let r = analyze(src, &registry()).unwrap();
        let usage = r
            .usages
            .iter()
            .find(|u| u.surface == "stores.MediaEngineStore")
            .unwrap();
        assert_eq!(usage.verdict, Verdict::Partial);
        assert!(usage.verdict.is_usable());
        assert_eq!(r.verdict(), "PORTABLE");
    }

    #[test]
    fn not_yet_implemented_surfaces_are_reported() {
        let src = r#"
export default definePlugin({
    name: "Voice",
    start() { VoiceStateStore.getVoiceState(); }
});
"#;
        let r = analyze(src, &registry()).unwrap();
        let usage = r
            .usages
            .iter()
            .find(|u| u.surface == "stores.VoiceStateStore")
            .unwrap();
        assert_eq!(usage.verdict, Verdict::NotYetImplemented);
        assert_eq!(r.verdict(), "PARTIAL - some surfaces not implemented yet");
        assert!(
            !r.has_blocking_issue(),
            "not-yet-implemented is a tracking gap, not a blocker"
        );
    }

    #[test]
    fn unknown_source_is_inconclusive() {
        let r = analyze("this is not a plugin", &registry()).unwrap();
        assert_eq!(
            r.verdict(),
            "INCONCLUSIVE - no recognisable API usage found"
        );
        assert!(!r.is_vendor_plugin);
    }

    #[test]
    fn extracts_quoted_name_field() {
        assert_eq!(
            extract_string_field(r#"  name: "MyPlugin","#, "name").as_deref(),
            Some("MyPlugin")
        );
        assert_eq!(
            extract_string_field("  name: 'Single',", "name").as_deref(),
            Some("Single")
        );
        assert_eq!(extract_string_field("  description: 'x',", "name"), None);
    }

    #[test]
    fn patch_counter_survives_nested_brackets_and_strings() {
        let src = r#"
patches: [
  { find: "a[b]", replacement: { a: 1 }, noWarn: true },
  { find: "c", replacement: "d" }
]
"#;
        let (found, count) = detect_patches(src);
        assert!(found);
        assert_eq!(count, 2);
    }

    #[test]
    fn render_groups_by_class() {
        let rendered = analyze(CLEAN_PLUGIN, &registry()).unwrap().render();
        assert!(rendered.contains("(a) data read"));
        assert!(rendered.contains("(b) data write"));
        assert!(rendered.contains("(d) ui injection"));
    }

    #[test]
    fn analyze_reads_a_file() {
        let dir = std::env::temp_dir().join("dcrs-port-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.ts");
        std::fs::write(&path, CLEAN_PLUGIN).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(analyze(&source, &registry()).is_ok());
        let _ = std::fs::remove_file(&path);
    }
}
