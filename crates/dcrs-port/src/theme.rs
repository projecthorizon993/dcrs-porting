//! Theme analysis: tier breakdown and translation coverage.

use std::collections::BTreeMap;

use dcrs_compat::Registry;
use dcrs_theme::{ClassMap, Theme};
use serde::Serialize;

/// Where a theme's CSS falls on the portability tiers.
///
/// Measured against real corpora, the distribution matters more than the total: a theme that is
/// 100% tier 1 is fully portable, one that is 100% tier 4 needs a class-map entry per selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// `:root` / `.theme-*` variable declarations.
    Variables,
    /// `var()` layering.
    VarRefs,
    /// `[class*="prefix"]` prefix matching, which works if the UI emits stable prefixes.
    Prefix,
    /// Fully hashed class names, which need a class-map entry.
    Hashed,
    /// Structural selectors: `:has()`, `:nth-child()`, `aria-*`.
    Structural,
}

impl Tier {
    /// Tier label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Variables => "variables",
            Self::VarRefs => "var-refs",
            Self::Prefix => "prefix",
            Self::Hashed => "hashed",
            Self::Structural => "structural",
        }
    }
}

/// The result of analyzing a theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeReport {
    /// Theme name.
    pub name: String,
    /// Author, from the `@author` header.
    pub author: Option<String>,
    /// Count per tier.
    pub tiers: BTreeMap<String, usize>,
    /// Hashed class names with no class-map entry.
    pub unmapped_classes: Vec<String>,
    /// Number of those that do have an entry.
    pub mapped_classes: usize,
    /// Variables declared.
    pub declared_variables: usize,
    /// Variables resolved successfully.
    pub resolved_variables: usize,
    /// Variables referenced but never declared.
    pub unresolved_variables: Vec<String>,
    /// Rules whose declarations change geometry, which are out of scope.
    pub geometry_rules: usize,
}

impl ThemeReport {
    /// Overall verdict.
    #[must_use]
    pub fn verdict(&self) -> &'static str {
        if self.declared_variables == 0 {
            return "INCONCLUSIVE - no variable declarations found";
        }
        let blocked = !self.unmapped_classes.is_empty();
        match (blocked, self.geometry_rules) {
            (false, 0) => "FULL - ships as variable overrides only",
            (false, _) => "MOSTLY FULL - variable overrides work; geometry rules need manual work",
            (true, 0) => "PARTIAL - needs class-map entries for unmapped hashed selectors",
            (true, _) => "PARTIAL - needs class-map entries and manual geometry work",
        }
    }

    /// Rendered as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("theme: {}\n", self.name));
        if let Some(author) = &self.author {
            out.push_str(&format!("  author: {author}\n"));
        }
        out.push_str(&format!(
            "  variables declared {} / resolved {}\n",
            self.declared_variables, self.resolved_variables
        ));
        if !self.unresolved_variables.is_empty() {
            out.push_str(&format!(
                "  unresolved vars: {}\n",
                self.unresolved_variables.join(", ")
            ));
        }
        out.push_str("  tier mix\n");
        for tier in [
            Tier::Variables,
            Tier::VarRefs,
            Tier::Prefix,
            Tier::Hashed,
            Tier::Structural,
        ] {
            let count = self.tiers.get(tier.as_str()).copied().unwrap_or(0);
            out.push_str(&format!("    {:<12} {count}\n", tier.as_str()));
        }
        out.push_str(&format!(
            "  class map     {}/{} hashed selectors mapped\n",
            self.mapped_classes,
            self.mapped_classes + self.unmapped_classes.len()
        ));
        if !self.unmapped_classes.is_empty() {
            let shown: Vec<&str> = self
                .unmapped_classes
                .iter()
                .take(10)
                .map(String::as_str)
                .collect();
            out.push_str(&format!("    unmapped: {}\n", shown.join(", ")));
            if self.unmapped_classes.len() > shown.len() {
                out.push_str(&format!(
                    "    ... and {} more\n",
                    self.unmapped_classes.len() - shown.len()
                ));
            }
        }
        out.push_str(&format!("  geometry rules {}\n", self.geometry_rules));
        out.push_str(&format!("  verdict: {}\n", self.verdict()));
        out
    }
}

/// Analyzes theme CSS against a class map.
#[must_use]
pub fn analyze(source: &str, map: &ClassMap) -> ThemeReport {
    // A malformed stylesheet still yields a useful report, so fall back rather than fail.
    let Ok(theme) = Theme::new("untitled", source) else {
        return ThemeReport {
            name: "untitled".to_owned(),
            author: None,
            tiers: BTreeMap::new(),
            unmapped_classes: vec![],
            mapped_classes: 0,
            declared_variables: 0,
            resolved_variables: 0,
            unresolved_variables: vec![],
            geometry_rules: 0,
        };
    };

    let mut tiers: BTreeMap<String, usize> = BTreeMap::new();
    let mut geometry_rules = 0usize;

    for rule in theme.stylesheet().rules() {
        if rule.is_variable_rule() {
            *tiers
                .entry(Tier::Variables.as_str().to_owned())
                .or_insert(0) += 1;
        }
        if rule
            .declarations
            .iter()
            .any(|d| dcrs_theme::css::extract_var_refs(&d.value).len() > 1)
        {
            *tiers.entry(Tier::VarRefs.as_str().to_owned()).or_insert(0) += 1;
        }
        if rule.declarations.iter().any(|d| is_geometry(&d.property)) {
            geometry_rules += 1;
        }
        for selector in &rule.selectors {
            if selector.contains(":has(")
                || selector.contains(":nth-child(")
                || selector.contains("[aria-")
            {
                *tiers
                    .entry(Tier::Structural.as_str().to_owned())
                    .or_insert(0) += 1;
            }
        }
    }

    let all = theme.all_classes();
    let unmapped = theme.unmapped_classes(map);
    let mapped_classes = all.iter().filter(|c| map.translate(c).is_some()).count();

    for class in &all {
        let entry = if dcrs_theme::classmap::looks_hashed(class) {
            if map.translate(class).is_some() {
                Tier::Hashed
            } else {
                continue;
            }
        } else {
            Tier::Prefix
        };
        *tiers.entry(entry.as_str().to_owned()).or_insert(0) += 1;
    }

    // Declared-variable count is available even when resolution fails, so the report can still
    // show that a theme is not empty.
    let declared = theme
        .stylesheet()
        .rules()
        .iter()
        .flat_map(|r| &r.declarations)
        .filter(|d| d.property.starts_with("--"))
        .count();

    let (resolved, unresolved) = if let Ok(vars) = theme.resolve(None) {
        (vars.len(), vars.unresolved().to_vec())
    } else {
        // A reference cycle is the only remaining failure mode; resolution is all-or-nothing.
        (0, vec![])
    };

    ThemeReport {
        name: theme.name().to_owned(),
        author: theme.author().map(str::to_owned),
        tiers,
        unmapped_classes: unmapped,
        mapped_classes,
        declared_variables: declared,
        resolved_variables: resolved,
        unresolved_variables: unresolved,
        geometry_rules,
    }
}

/// Properties that change box geometry and therefore cannot be honoured by a value-only engine.
fn is_geometry(property: &str) -> bool {
    matches!(
        property,
        "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "position"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "transform"
            | "order"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "gap"
            | "flex"
            | "flex-direction"
            | "grid-template"
            | "display"
            | "inset"
    )
}

/// The registry's coverage, as JSON.
#[must_use]
pub fn coverage_json(registry: &Registry) -> serde_json::Value {
    serde_json::json!({
        "surfaces": registry.len(),
        "coverage": registry.coverage(),
        "by_class": registry.class_histogram()
            .iter()
            .map(|(k, v)| (k.label().to_owned(), v))
            .collect::<std::collections::BTreeMap<_, _>>(),
    })
}

/// The registry's coverage, as text.
#[must_use]
pub fn render_coverage(registry: &Registry) -> String {
    let mut out = String::new();
    out.push_str("capability registry\n");
    out.push_str(&format!("  surfaces   {}\n", registry.len()));
    out.push_str(&format!(
        "  coverage   {:.1}% of replicable surfaces\n",
        registry.coverage() * 100.0
    ));
    out.push_str("  by class\n");
    let hist = registry.class_histogram();
    for (class, count) in &hist {
        out.push_str(&format!(
            "    ({}) {:<16} {count:>4}  {}\n",
            class.as_str(),
            class.label(),
            if class.is_replicable() {
                "replicable"
            } else {
                "NOT replicable"
            }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcrs_compat::{Class, Support};

    #[test]
    fn variable_only_theme_is_full() {
        let src = ":root { --background-primary: #313338; --text-normal: #fff; }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.declared_variables, 2);
        assert_eq!(r.resolved_variables, 2);
        assert_eq!(r.verdict(), "FULL - ships as variable overrides only");
        assert!(
            r.render().contains("variables"),
            "tier table missing:\n{}",
            r.render()
        );
    }

    #[test]
    fn unmapped_hashed_selectors_are_reported() {
        let src =
            ":root { --a: 1px; } .channel-2f1c9d { --b: 2px; } .chatContent-31rq { --c: 3px; }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.unmapped_classes.len(), 2);
        assert!(r.verdict().starts_with("PARTIAL"));
        assert!(r.render().contains("channel-2f1c9d"));
    }

    #[test]
    fn mapped_hashed_selectors_are_translated() {
        use dcrs_theme::classmap::{ClassEntry, Surface as MapSurface};
        let map = ClassMap::from_entries(
            std::collections::BTreeMap::from([(
                "channel-2f1c9d".to_owned(),
                ClassEntry {
                    stable: "channel".to_owned(),
                    surface: MapSurface::ChannelList,
                    notes: None,
                },
            )]),
            None,
        )
        .unwrap();
        let r = analyze(":root { --a: 1px; } .channel-2f1c9d { --b: 2px; }", &map);
        assert!(
            r.unmapped_classes.is_empty(),
            "expected none, got {:?}",
            r.unmapped_classes
        );
        assert_eq!(r.mapped_classes, 1);
        assert_eq!(r.verdict(), "FULL - ships as variable overrides only");
    }

    #[test]
    fn geometry_rules_are_counted() {
        let src = ":root { --a: 1px; } .x { width: 100px; height: 50px; position: absolute; }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.geometry_rules, 1);
        assert!(r.verdict().contains("geometry rules need manual work"));
    }

    #[test]
    fn structural_selectors_are_counted() {
        let src = ":root { --a: 1px; } .x:has(.y) { --b: 2px; } .z:nth-child(3) { --c: 3px; }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.tiers.get("structural"), Some(&2));
    }

    #[test]
    fn var_layering_is_counted() {
        let src = ":root { --a: var(--b, 1) var(--c, 2); --b: 3px; --c: 4px; }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.tiers.get("var-refs"), Some(&1));
    }

    #[test]
    fn unresolved_vars_are_listed_not_fatal() {
        let src = ":root { --a: 1px; --b: var(--vc-injected, 2px); }";
        let r = analyze(src, &ClassMap::default());
        assert_eq!(r.unresolved_variables, vec!["--vc-injected".to_owned()]);
        assert_eq!(r.verdict(), "FULL - ships as variable overrides only");
    }

    #[test]
    fn unparseable_css_still_produces_a_report() {
        let r = analyze("}}}{{{", &ClassMap::default());
        assert_eq!(r.declared_variables, 0);
        assert_eq!(r.verdict(), "INCONCLUSIVE - no variable declarations found");
    }

    #[test]
    fn coverage_renders_and_serializes() {
        let registry = Registry::from_surfaces(vec![
            dcrs_compat::Surface {
                id: "stores.X".into(),
                class: Class::Read,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            dcrs_compat::Surface {
                id: "internals.Y".into(),
                class: Class::Internals,
                support: Support::Unsupported,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
        ])
        .unwrap();
        let text = render_coverage(&registry);
        assert!(text.contains("surfaces   2"));
        assert!(text.contains("NOT replicable"));

        let json = coverage_json(&registry);
        assert_eq!(json["surfaces"], 2);
        assert!(json["by_class"]["data read"].is_number());
    }

    #[test]
    fn is_geometry_covers_the_common_cases() {
        for p in ["width", "position", "transform", "gap", "display"] {
            assert!(is_geometry(p), "{p} should count as geometry");
        }
        for p in [
            "color",
            "background-color",
            "border-radius",
            "--main-color",
            "font-size",
        ] {
            assert!(!is_geometry(p), "{p} should not count as geometry");
        }
    }
}
