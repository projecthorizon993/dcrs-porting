//! Loading a theme from CSS: manifest scraping, class-map translation, and variable access.

use crate::classmap::ClassMap;
use crate::css::{self, ParseError, ResolveError, Stylesheet, ThemeKind, Variables};

/// A theme loaded from source, ready to be applied to a client.
#[derive(Debug, Clone)]
pub struct Theme {
    name: String,
    author: Option<String>,
    description: Option<String>,
    version: Option<String>,
    source: String,
    sheet: Stylesheet,
}

impl Theme {
    /// Builds a theme from a display name and its CSS source.
    ///
    /// # Errors
    /// Returns an error if the CSS cannot be parsed.
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Result<Self, ParseError> {
        let source = source.into();
        let manifest = parse_manifest(&source);
        Ok(Self {
            name: name.into(),
            author: manifest.author,
            description: manifest.description,
            version: manifest.version,
            sheet: Stylesheet::parse(&source)?,
            source,
        })
    }

    /// Theme display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Theme author, from an `@author` header if present.
    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Theme description, from a `@description` header if present.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Theme version, from a `@version` header if present.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The original CSS source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The parsed stylesheet.
    #[must_use]
    pub fn stylesheet(&self) -> &Stylesheet {
        &self.sheet
    }

    /// Resolves this theme's variables for a target theme kind.
    ///
    /// # Errors
    /// Returns an error on an undefined `var()` with no fallback, or a reference cycle.
    pub fn resolve(&self, kind: Option<ThemeKind>) -> Result<Variables, ResolveError> {
        Variables::resolve(&self.sheet, kind)
    }

    /// Class names the theme references that have no mapping in `map`.
    ///
    /// This is the count the porting tool reports as untranslated, and it is the maintenance
    /// signal that says when to re-scrape the class map.
    #[must_use]
    pub fn unmapped_classes(&self, map: &ClassMap) -> Vec<String> {
        let mut unmapped: Vec<String> = self
            .sheet
            .rules()
            .iter()
            .flat_map(|r| r.selectors.iter())
            .flat_map(|sel| class_names_in(sel))
            .filter(|c| crate::classmap::looks_hashed(c) && map.translate(c).is_none())
            .collect();
        unmapped.sort();
        unmapped.dedup();
        unmapped
    }

    /// Every class name the theme references, mapped and unmapped.
    #[must_use]
    pub fn all_classes(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .sheet
            .rules()
            .iter()
            .flat_map(|r| r.selectors.iter())
            .flat_map(|sel| class_names_in(sel))
            .collect();
        all.sort();
        all.dedup();
        all
    }

    /// Returns a copy of this theme with hashed selectors rewritten through `map`.
    ///
    /// Unmapped hashed names are left as-is, so the porting tool can still report them.
    ///
    /// # Errors
    /// Returns an error if the rewritten CSS cannot be parsed.
    pub fn translated(&self, map: &ClassMap) -> Result<Self, ParseError> {
        let rewritten = self
            .sheet
            .rules()
            .iter()
            .map(|rule| {
                let selectors = rule
                    .selectors
                    .iter()
                    .map(|s| css::translate_selector(s, map))
                    .collect::<Vec<_>>();
                css::Rule {
                    selectors,
                    declarations: rule.declarations.clone(),
                    order: rule.order,
                }
            })
            .map(|rule| render_rule(&rule))
            .collect::<Vec<_>>()
            .join("\n");

        Theme::new(self.name.clone(), rewritten)
    }
}

/// Metadata scraped from a theme's comment headers.
#[derive(Debug, Default, PartialEq, Eq)]
struct Manifest {
    author: Option<String>,
    description: Option<String>,
    version: Option<String>,
}

/// Scans the leading comment block for `@key value` headers.
///
/// `BetterDiscord` themes conventionally put this at the top of the file, either as
/// `/* @name X • @author Y • @version 1.2 */` or as one `@key` per line. Both are accepted.
fn parse_manifest(source: &str) -> Manifest {
    let Some(header_end) = source.find("*/") else {
        return Manifest::default();
    };
    let header = &source[..header_end];
    let mut manifest = Manifest::default();

    for (key, target) in [
        ("@author", &mut manifest.author),
        ("@description", &mut manifest.description),
        ("@version", &mut manifest.version),
    ] {
        if let Some(value) = header_value(header, key) {
            *target = Some(value);
        }
    }
    manifest
}

/// Extracts the value following `key` up to the next bullet separator or end of line.
fn header_value(header: &str, key: &str) -> Option<String> {
    let start = header.find(key)? + key.len();
    let rest = &header[start..];
    let stop = rest.find(['\n', '\r', '\u{2022}']).unwrap_or(rest.len());
    let value = rest[..stop].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// Extracts class names from a selector, unescaping CSS escapes.
fn class_names_in(selector: &str) -> impl Iterator<Item = String> + use<'_> {
    let bytes = selector.as_bytes();
    let mut names = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric()
                    || bytes[j] == b'-'
                    || bytes[j] == b'_'
                    || bytes[j] == b'\\')
            {
                j += 1;
            }
            if j > start {
                names.push(selector[start..j].replace('\\', ""));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    names.into_iter()
}

/// Renders a rule back to CSS text.
///
/// Declaration values may contain commas (font stacks, `rgba()`), but those are inside a
/// declaration block and the re-parse only splits selectors at the top level before `{`, so a
/// naive join round-trips correctly.
fn render_rule(rule: &css::Rule) -> String {
    let mut out = String::new();
    out.push_str(&rule.selectors.join(", "));
    out.push_str(" { ");
    for (i, decl) in rule.declarations.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        out.push_str(&decl.property);
        out.push_str(": ");
        out.push_str(&decl.value);
        if decl.important {
            out.push_str(" !important");
        }
    }
    out.push_str(" }");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classmap::{ClassEntry, Surface as MapSurface};

    const SIMPLE: &str = "/* @name Test • @author someone • @version 1.2 */\n:root { --background-primary: #313338; }";

    fn map_with_channel() -> ClassMap {
        ClassMap::from_entries(
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
        .unwrap()
    }

    #[test]
    fn reads_manifest_headers() {
        let theme = Theme::new("Test", SIMPLE).unwrap();
        assert_eq!(theme.author(), Some("someone"));
        assert_eq!(theme.version(), Some("1.2"));
    }

    #[test]
    fn reads_multiline_manifest_headers() {
        let src =
            "/*\n@name Multi\n@author someone\n@description a theme\n@version 2.0\n*/\n:root{a:1}";
        let theme = Theme::new("Multi", src).unwrap();
        assert_eq!(theme.author(), Some("someone"));
        assert_eq!(theme.description(), Some("a theme"));
        assert_eq!(theme.version(), Some("2.0"));
    }

    #[test]
    fn works_without_manifest() {
        let theme = Theme::new("Bare", ":root { --a: 1px; }").unwrap();
        assert_eq!(theme.author(), None);
        assert_eq!(theme.version(), None);
        assert_eq!(theme.name(), "Bare");
    }

    #[test]
    fn resolves_variables() {
        let theme = Theme::new("T", ":root { --a: var(--b, #fff); }").unwrap();
        let vars = theme.resolve(None).unwrap();
        assert_eq!(vars.value("--a"), Some("#fff"));
    }

    #[test]
    fn lists_unmapped_hashed_classes() {
        let map = ClassMap::default();
        let theme = Theme::new(
            "T",
            ":root { --a: 1; } .channel-2f1c9d { --b: 2; } .chatContent-31rq { --c: 3; } .stable { --d: 4; }",
        )
        .unwrap();
        assert_eq!(
            theme.unmapped_classes(&map),
            vec!["channel-2f1c9d".to_owned(), "chatContent-31rq".to_owned()]
        );
    }

    #[test]
    fn translation_rewrites_mapped_names_only() {
        let theme = Theme::new(
            "T",
            ":root { --a: 1; } .channel-2f1c9d { --b: 2; } .chatContent-31rq { --c: 3; }",
        )
        .unwrap();
        let translated = theme.translated(&map_with_channel()).unwrap();
        let classes = translated.all_classes();
        assert!(classes.contains(&"channel".to_owned()));
        assert!(!classes.contains(&"channel-2f1c9d".to_owned()));
        // The still-unmapped class survives so the report can find it.
        assert!(classes.contains(&"chatContent-31rq".to_owned()));
    }

    #[test]
    fn translated_theme_keeps_declarations() {
        let theme = Theme::new(
            "T",
            ":root { --background-primary: red; } .channel-2f1c9d { --b: 2px; }",
        )
        .unwrap();
        let translated = theme.translated(&map_with_channel()).unwrap();
        let vars = translated.resolve(None).unwrap();
        assert_eq!(vars.value("--background-primary"), Some("red"));
        assert!(translated.source().contains(".channel {"));
    }

    #[test]
    fn element_scoped_variables_are_not_global() {
        // `--a` under `.channel` applies to that element only, so it is not a global variable
        // and must not appear in the resolved set.
        let theme = Theme::new(
            "T",
            ".channel { --local-only: 1px; } :root { --global: 2px; }",
        )
        .unwrap();
        let vars = theme.resolve(None).unwrap();
        assert_eq!(vars.value("--global"), Some("2px"));
        assert_eq!(vars.value("--local-only"), None);
    }

    #[test]
    fn translation_preserves_important() {
        let theme = Theme::new("T", ".channel-2f1c9d { --a: red !important; }").unwrap();
        let translated = theme.translated(&map_with_channel()).unwrap();
        assert!(translated.source().contains("!important"));
    }

    #[test]
    fn all_classes_dedupes_and_sorts() {
        let theme = Theme::new("T", ".b{x:1} .a{y:1} .b{z:1}").unwrap();
        assert_eq!(theme.all_classes(), vec!["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn rejects_unterminated_css() {
        assert!(Theme::new("T", ":root { --a: 1px;").is_err());
    }
}
