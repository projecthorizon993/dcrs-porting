//! Parsing and token resolution for the CSS subset that Discord themes actually use.
//!
//! Measured against four real theme corpora, shipped themes are dominated by `:root` /
//! `.theme-dark` variable declarations plus `var()` layering. Geometry-changing rules are out of
//! scope: this engine resolves *values*, it does not implement a CSS box model.
//!
//! Supported on purpose: custom property declarations, `var()` references, the `--x-hsl` triple
//! form, `calc()` in lightness position, `.theme-dark` / `.theme-light` scoping, and class-name
//! translation through a [`ClassMap`].

use std::collections::BTreeMap;

use crate::classmap::ClassMap;

/// A raw declaration list for one variable: `(source order, is-important, raw value)`.
type RawDecls = Vec<(usize, bool, String)>;

/// All candidate declarations of all variables, keyed by scope and name.
type Candidates = BTreeMap<(Scope, String), RawDecls>;

/// One `property: value` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Property name, lowercased. Custom properties keep their leading `--`.
    pub property: String,
    /// Raw value text, whitespace-trimmed, comments stripped.
    pub value: String,
    /// Whether `!important` was present.
    pub important: bool,
}

impl Declaration {
    /// Builds a non-important declaration.
    #[must_use]
    pub fn new(property: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            property: property.into(),
            value: value.into(),
            important: false,
        }
    }
}

/// A parsed rule: a selector list plus its declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Comma-separated selectors, whitespace-normalized.
    pub selectors: Vec<String>,
    /// The rule's declarations.
    pub declarations: Vec<Declaration>,
    /// Source order index, used for cascade tie-breaking.
    pub order: usize,
}

impl Rule {
    /// Whether this rule targets a custom property declaration.
    #[must_use]
    pub fn is_variable_rule(&self) -> bool {
        self.declarations
            .iter()
            .any(|d| d.property.starts_with("--"))
    }

    /// Scope selectors this rule applies to, based on which theme root classes it carries.
    ///
    /// Recognised roots mirror the ones themes ship: `:root`, `.theme-dark`, `.theme-light`,
    /// `.theme-darker`, `.theme-midnight`, `.theme-brand`, `.custom-theme-background`.
    #[must_use]
    pub fn scopes(&self) -> Vec<Scope> {
        let mut scopes = Vec::new();
        for selector in &self.selectors {
            let s = selector.trim();
            if s == ":root" || s == "html" {
                scopes.push(Scope::Root);
            } else if let Some(rest) = s.strip_prefix('.') {
                if let Some(theme) = rest.strip_prefix("theme-") {
                    if let Some(kind) = ThemeKind::parse(theme) {
                        scopes.push(Scope::Theme(kind));
                    }
                }
            }
        }
        scopes.dedup();
        scopes
    }
}

/// Which theme a scope block applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThemeKind {
    /// `.theme-dark`, the default Discord dark theme.
    Dark,
    /// `.theme-light`.
    Light,
    /// `.theme-darker`.
    Darker,
    /// `.theme-midnight`.
    Midnight,
    /// `.theme-brand`, a preset.
    Brand,
    /// `.custom-theme-background`, set when the user applies a custom background.
    CustomBackground,
}

impl ThemeKind {
    /// Parses a theme name from a `.theme-*` class suffix.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "darker" => Some(Self::Darker),
            "midnight" => Some(Self::Midnight),
            "brand" => Some(Self::Brand),
            _ => None,
        }
    }

    /// CSS class that activates this theme.
    #[must_use]
    pub const fn css_class(self) -> &'static str {
        match self {
            Self::Dark => "theme-dark",
            Self::Light => "theme-light",
            Self::Darker => "theme-darker",
            Self::Midnight => "theme-midnight",
            Self::Brand => "theme-brand",
            Self::CustomBackground => "custom-theme-background",
        }
    }
}

/// The scope a variable declaration applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Unscoped: applies to every theme.
    Root,
    /// Applies only to one theme.
    Theme(ThemeKind),
}

/// A parsed stylesheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stylesheet {
    rules: Vec<Rule>,
}

/// Parse failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    /// A declaration was missing its colon.
    #[error("malformed declaration at byte {offset}: {text:?}")]
    MalformedDeclaration {
        /// Byte offset in the input.
        offset: usize,
        /// Offending text.
        text: String,
    },
    /// A rule was missing its opening brace.
    #[error("unterminated rule at byte {offset}")]
    UnterminatedRule {
        /// Byte offset in the input.
        offset: usize,
    },
    /// A `var()` reference had no fallback and the variable was undefined.
    #[error("undefined variable {name:?} with no fallback")]
    UndefinedVariable {
        /// The variable name, including `--`.
        name: String,
    },
}

impl Stylesheet {
    /// Parses a stylesheet from CSS source.
    ///
    /// This is a tolerant parser: it handles nested at-rules and comments by flattening, and
    /// skips any declaration it cannot make sense of rather than failing the whole file.
    /// Real-world themes contain vendor prefixes and syntax the native client will never support,
    /// and a theme that mostly works is more useful than a hard error.
    ///
    /// # Errors
    /// Returns [`ParseError::UnterminatedRule`] if the input ends inside a rule.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        // A byte-order mark would otherwise be glued to the first selector, turning `:root` into
        // something unrecognised and silently dropping every root-scoped declaration. Files saved by
        // Windows editors routinely carry one.
        let stripped = strip_comments(input.trim_start_matches('\u{feff}'));
        let mut rules = Vec::new();
        let mut order = 0usize;

        for (selector_block, body, _offset) in split_rules(&stripped)? {
            let selectors: Vec<String> = selector_block
                .split(',')
                .map(normalize_whitespace)
                .filter(|s| !s.is_empty())
                .collect();
            if selectors.is_empty() {
                continue;
            }

            let declarations = parse_declarations(&body);
            if declarations.is_empty() {
                continue;
            }

            rules.push(Rule {
                selectors,
                declarations,
                order,
            });
            order += 1;
        }

        Ok(Self { rules })
    }

    /// All rules, in source order.
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Number of rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the stylesheet holds no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Parses the declarations inside a rule body.
fn parse_declarations(body: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for decl in split_declarations(body) {
        let Some((property, rest)) = decl.split_once(':') else {
            continue; // tolerate junk rather than failing the theme
        };
        let property = normalize_property(property);
        if property.is_empty() {
            continue;
        }
        let (value, important) = split_important(rest);
        let value = normalize_whitespace(value);
        if value.is_empty() {
            continue;
        }
        out.push(Declaration {
            property,
            value,
            important,
        });
    }
    out
}

/// Removes `/* ... */` comments, preserving byte offsets by replacing content with spaces.
fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start = i;
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            // Preserve length so reported offsets stay meaningful.
            out.extend(std::iter::repeat_n(' ', i - start));
        } else {
            let ch_len = utf8_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            out.push_str(&input[i..end]);
            i = end;
        }
    }
    out
}

/// Length in bytes of the UTF-8 sequence starting with `first`.
fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Splits top-level rules, flattening one level of at-rule nesting.
///
/// Returns `(selector_block, body, offset)` per rule. Nested at-rules such as `@media` are
/// flattened by appending the condition to each selector, since the engine has no media query
/// evaluation.
fn split_rules(input: &str) -> Result<Vec<(String, String, usize)>, ParseError> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    // Where the current selector prelude begins.
    let mut sel_start = 0usize;
    let mut i = 0usize;
    // Preceding at-rules, so nested rules can inherit their conditions.
    let mut at_rules: Vec<String> = Vec::new();

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                let prelude = normalize_whitespace(&input[sel_start..i]);
                let body_start = i + 1;

                if prelude.starts_with('@') {
                    // An at-rule: its contents may hold style rules, so keep scanning inside it.
                    at_rules.push(prelude);
                    sel_start = body_start;
                    i = body_start;
                    continue;
                }

                let (body_end, after) = find_block_end(bytes, i)
                    .ok_or(ParseError::UnterminatedRule { offset: sel_start })?;
                let body = input[body_start.min(body_end)..body_end].to_owned();

                let mut selectors = vec![prelude];
                for at in &at_rules {
                    let cond = at.trim_start_matches('@').trim();
                    if !cond.is_empty() {
                        selectors = selectors
                            .iter()
                            .map(|s| format!("{s} /* {cond} */"))
                            .collect();
                    }
                }

                let offset = sel_start;
                for sel in selectors {
                    if !sel.is_empty() {
                        out.push((sel, body.clone(), offset));
                    }
                }

                at_rules.clear();
                i = after;
                sel_start = after;
            }
            // A `;` terminates a prelude (an at-rule with no body); a stray `}` closes a level.
            b';' | b'}' => {
                sel_start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }

    Ok(out)
}

/// Given the index of an opening brace, returns the index of its closing brace and the index
/// after it, or `None` if the block is never closed.
fn find_block_end(bytes: &[u8], open: usize) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((i, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Splits a declaration body on top-level semicolons, respecting parentheses and quotes.
fn split_declarations(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut start = 0usize;

    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if b == q && bytes.get(i.wrapping_sub(1)) != Some(&b'\\') {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => {
                parts.push(body[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < body.len() {
        parts.push(body[start..].to_owned());
    }
    parts
}

/// Splits `!important` off the end of a value.
fn split_important(value: &str) -> (&str, bool) {
    let lower = value.to_ascii_lowercase();
    match lower.rfind("!important") {
        Some(idx) if idx + 10 == lower.trim_end().len() => (value[..idx].trim_end(), true),
        _ => (value.trim(), false),
    }
}

/// Lowercases a property name and normalizes leading whitespace.
fn normalize_property(raw: &str) -> String {
    normalize_whitespace(raw).to_ascii_lowercase()
}

/// Collapses runs of whitespace to single spaces and trims.
fn normalize_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A `var(--name, fallback)` reference found in a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef {
    /// Referenced variable name, including `--`.
    pub name: String,
    /// Fallback expression if the variable is undefined.
    pub fallback: Option<String>,
}

/// Extracts `var()` references from a value, in order of appearance.
#[must_use]
pub fn extract_var_refs(value: &str) -> Vec<VarRef> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if value[i..].starts_with("var(") {
            let open = i + 4;
            let mut depth = 1usize;
            let mut j = open;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let inner = &value[open..j.min(value.len())];
            let (name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim(), Some(normalize_whitespace(f))),
                None => (inner.trim(), None),
            };
            if name.starts_with("--") {
                out.push(VarRef {
                    name: name.to_owned(),
                    fallback: fallback.filter(|f| !f.is_empty()),
                });
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Rewrites every hashed class name in a selector to its stable equivalent.
///
/// Class selectors are rewritten in place. Hashed names appearing in attribute selectors such as
/// `[class*="channel-"]` are also rewritten, which is what makes the legacy `BetterDiscord`
/// `[class*=…]` corpus work against a native client that emits stable names.
#[must_use]
pub fn translate_selector(selector: &str, map: &ClassMap) -> String {
    let mut out = selector.to_owned();
    // Longest hashed names first, so a short name cannot partially rewrite a longer one.
    let mut pairs: Vec<(&str, &str)> = map.iter().collect();
    pairs.sort_by_key(|(hashed, _)| std::cmp::Reverse(hashed.len()));

    for (hashed, stable) in pairs {
        if stable == hashed {
            continue;
        }
        // `.hashed` selector form.
        out = out.replace(&format!(".{hashed}"), &format!(".{stable}"));
        // `[class*="hashed"]` and `[class^="hashed"]` attribute forms.
        for op in ["*=", "^=", "$=", "="] {
            out = out.replace(
                &format!("[class{op}\"{hashed}"),
                &format!("[class{op}\"{stable}"),
            );
            out = out.replace(
                &format!("[class{op}'{hashed}"),
                &format!("[class{op}'{stable}"),
            );
        }
    }
    out
}

/// A resolved variable: its final value and the scope it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedVar {
    /// Final value with all `var()` references expanded.
    pub value: String,
    /// Where the declaration came from.
    pub scope: Scope,
    /// Source order of the winning declaration.
    pub order: usize,
}

/// A variable resolution failure.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResolveError {
    /// A referenced variable was undefined and had no fallback.
    #[error("undefined variable {name:?} (referenced by {referrer:?})")]
    Undefined {
        /// The undefined variable.
        name: String,
        /// The value that referenced it.
        referrer: String,
    },
    /// A variable referenced itself, directly or through a cycle.
    #[error("cyclic variable reference involving {0}")]
    Cycle(String),
}

/// The result of resolving a theme's variables.
#[derive(Debug, Clone, Default)]
pub struct Variables {
    resolved: BTreeMap<String, ResolvedVar>,
    unresolved: Vec<String>,
}

impl Variables {
    /// Resolves every custom property declaration in a stylesheet for a given theme scope.
    ///
    /// Cascade order: root-scope declarations first, then the theme's own, then later source order
    /// wins. `!important` wins over non-important regardless of order.
    ///
    /// # Errors
    /// Returns an error on an undefined `var()` with no fallback, or a reference cycle.
    pub fn resolve(
        stylesheet: &Stylesheet,
        theme: Option<ThemeKind>,
    ) -> Result<Self, ResolveError> {
        // Collect candidates per (scope, variable).
        let mut candidates: Candidates = BTreeMap::new();

        for rule in stylesheet.rules() {
            for decl in &rule.declarations {
                if !decl.property.starts_with("--") {
                    continue;
                }
                for scope in rule.scopes() {
                    let applies = match scope {
                        Scope::Root => true,
                        Scope::Theme(kind) => theme == Some(kind),
                    };
                    if !applies {
                        continue;
                    }
                    candidates
                        .entry((scope, decl.property.clone()))
                        .or_default()
                        .push((rule.order, decl.important, decl.value.clone()));
                }
            }
        }

        let mut resolved = BTreeMap::new();
        let mut unresolved: Vec<String> = Vec::new();
        for ((scope, name), list) in &candidates {
            // Candidates are `(order, important, raw)`; highest `important` wins, then latest.
            let Some((order, _, raw)) = list
                .iter()
                .max_by_key(|&(order, important, _)| (important, order))
            else {
                continue;
            };
            let (order, raw) = (*order, raw.clone());
            let value = expand(name, &raw, &candidates, &mut unresolved, &mut Vec::new())?;
            resolved.insert(
                name.clone(),
                ResolvedVar {
                    value,
                    scope: *scope,
                    order,
                },
            );
        }

        // Variables referenced but never declared anywhere in the stylesheet. Themes routinely rely
        // on host-injected variables, so these are reported rather than treated as errors.
        let declared: std::collections::BTreeSet<String> = resolved.keys().cloned().collect();
        for list in candidates.values() {
            for (_, _, raw) in list {
                for var_ref in extract_var_refs(raw) {
                    if !declared.contains(&var_ref.name) {
                        unresolved.push(var_ref.name.clone());
                    }
                }
            }
        }
        unresolved.sort();
        unresolved.dedup();

        Ok(Self {
            resolved,
            unresolved,
        })
    }

    /// Looks up a resolved variable.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ResolvedVar> {
        self.resolved.get(name)
    }

    /// Value of a variable, or `None`.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.resolved.get(name).map(|v| v.value.as_str())
    }

    /// All resolved variables.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ResolvedVar)> {
        self.resolved.iter()
    }

    /// Number of resolved variables.
    #[must_use]
    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    /// Whether nothing resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    /// Variables referenced but never declared, sorted and deduplicated.
    #[must_use]
    pub fn unresolved(&self) -> &[String] {
        &self.unresolved
    }

    /// The `--x-hsl` companion form of an `--x` variable, if both exist.
    ///
    /// Themes use these interchangeably: `--background-primary` for direct colour use and
    /// `--background-primary-hsl` inside `hsl(…)` compositions.
    #[must_use]
    pub fn hsl_of(&self, name: &str) -> Option<&str> {
        self.value(&format!("{name}-hsl"))
    }
}

/// Recursively expands `var()` references in a value.
///
/// A reference to an undeclared variable with no fallback is left in place verbatim and recorded
/// in `unresolved`, rather than failing: real themes depend on variables the host injects, and a
/// single such reference must not discard the rest of the theme.
fn expand(
    name: &str,
    value: &str,
    candidates: &Candidates,
    unresolved: &mut Vec<String>,
    stack: &mut Vec<String>,
) -> Result<String, ResolveError> {
    if stack.contains(&name.to_owned()) {
        return Err(ResolveError::Cycle(name.to_owned()));
    }
    stack.push(name.to_owned());

    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0usize;
    let mut last = 0usize;

    while i < bytes.len() {
        if value[i..].starts_with("var(") {
            out.push_str(&value[last..i]);
            let open = i + 4;
            let mut depth = 1usize;
            let mut j = open;
            while j < bytes.len() {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let inner = &value[open..j.min(value.len())];
            let (var_name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim(), Some(normalize_whitespace(f))),
                None => (inner.trim(), None),
            };

            match lookup(candidates, var_name) {
                Some(raw) => {
                    let expanded = expand(var_name, &raw, candidates, unresolved, stack)?;
                    out.push_str(&expanded);
                }
                None => {
                    if let Some(f) = fallback {
                        out.push_str(&f);
                    } else {
                        // Keep the reference verbatim so the value stays traceable.
                        unresolved.push(var_name.to_owned());
                        out.push_str(&value[i..(j + 1).min(value.len())]);
                    }
                }
            }
            i = j + 1;
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[last..]);
    stack.pop();
    Ok(out)
}

/// Finds the winning declaration for a variable across all scopes.
///
/// Candidates are stored as `(order, important, raw)`.
fn lookup(candidates: &Candidates, name: &str) -> Option<String> {
    let mut best: Option<(bool, usize, String)> = None;
    for ((_scope, key), list) in candidates {
        if key != name {
            continue;
        }
        let Some(&(order, important, ref raw)) = list
            .iter()
            .max_by_key(|&(order, important, _)| (important, order))
        else {
            continue;
        };
        let candidate = (important, order, raw.clone());
        best = match best {
            Some(prev) if (prev.0, prev.1) >= (candidate.0, candidate.1) => Some(prev),
            _ => Some(candidate),
        };
    }
    best.map(|(_, _, raw)| raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classmap::{ClassEntry, Surface};

    fn parse(src: &str) -> Stylesheet {
        Stylesheet::parse(src).expect("valid css")
    }

    #[test]
    fn parses_simple_rules() {
        let sheet = parse(":root { --main-color: #00ff00; --spacing-8: 8px; }");
        assert_eq!(sheet.len(), 1);
        let rule = &sheet.rules()[0];
        assert_eq!(rule.selectors, vec![":root"]);
        assert_eq!(rule.declarations.len(), 2);
        assert_eq!(rule.declarations[0].property, "--main-color");
        assert_eq!(rule.declarations[0].value, "#00ff00");
    }

    #[test]
    fn strips_comments_and_normalizes_whitespace() {
        let sheet = parse("/* header */\n:root {\n  --a:   1px;\n  /* mid */\n  --b: 2px;\n}");
        assert_eq!(sheet.len(), 1);
        assert_eq!(sheet.rules()[0].declarations.len(), 2);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_root_block() {
        // Windows editors save with a BOM, and without this the first selector parses as
        // `\u{feff} :root`, which matches no scope — so every root declaration vanishes silently.
        let sheet = parse("\u{feff}:root { --a: #111111; }");
        assert_eq!(sheet.rules()[0].scopes(), vec![Scope::Root]);
    }

    #[test]
    fn detects_important() {
        let sheet = parse(":root { --a: red; --b: blue !important; }");
        let decls = &sheet.rules()[0].declarations;
        assert!(!decls[0].important);
        assert!(decls[1].important);
        assert_eq!(decls[1].value, "blue");
    }

    #[test]
    fn preserves_semicolons_inside_strings_and_parens() {
        let sheet = parse(r#":root { --a: "x;y"; --b: calc(1px; 2px); }"#);
        let decls = &sheet.rules()[0].declarations;
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].value, "\"x;y\"");
    }

    #[test]
    fn handles_nested_at_rules() {
        let sheet = parse("@media (min-width: 100px) { :root { --a: 1px; } }");
        assert_eq!(sheet.len(), 1);
        assert!(sheet.rules()[0].selectors[0].starts_with(":root"));
        assert_eq!(sheet.rules()[0].declarations[0].property, "--a");
    }

    #[test]
    fn multiple_rules_keep_source_order() {
        let sheet = parse(":root { --a: 1; } :root { --b: 2; } :root { --c: 3; }");
        assert_eq!(sheet.len(), 3);
        assert_eq!(sheet.rules()[0].order, 0);
        assert_eq!(sheet.rules()[2].order, 2);
    }

    #[test]
    fn tolerates_trailing_junk() {
        let sheet = parse(":root { --a: 1 } garbage");
        assert_eq!(sheet.len(), 1);
    }

    #[test]
    fn reports_unterminated_rule() {
        // A rule opened but never closed must not silently produce a partial stylesheet.
        let err = Stylesheet::parse(":root { --a: 1px;").unwrap_err();
        assert!(matches!(err, ParseError::UnterminatedRule { .. }));
    }

    #[test]
    fn identifies_scopes() {
        let sheet = parse(":root{a:1} .theme-dark{b:2} .theme-light{c:3}");
        assert_eq!(sheet.rules()[0].scopes(), vec![Scope::Root]);
        assert_eq!(
            sheet.rules()[1].scopes(),
            vec![Scope::Theme(ThemeKind::Dark)]
        );
        assert_eq!(
            sheet.rules()[2].scopes(),
            vec![Scope::Theme(ThemeKind::Light)]
        );
    }

    #[test]
    fn extracts_var_refs_with_and_without_fallback() {
        let refs = extract_var_refs("var(--a) var(--b, 10px)");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "--a");
        assert_eq!(refs[0].fallback, None);
        assert_eq!(refs[1].name, "--b");
        assert_eq!(refs[1].fallback.as_deref(), Some("10px"));
    }

    #[test]
    fn resolves_var_chains() {
        let sheet =
            parse(":root { --base: #5865f2; --accent: var(--base); --final: var(--accent); }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.value("--final"), Some("#5865f2"));
        assert_eq!(vars.value("--accent"), Some("#5865f2"));
    }

    #[test]
    fn resolves_var_with_fallback() {
        let sheet = parse(":root { --a: var(--missing, 42px); }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.value("--a"), Some("42px"));
    }

    #[test]
    fn undefined_var_without_fallback_is_preserved_and_reported() {
        // A theme depending on a host-injected variable must still resolve; the reference is kept
        // verbatim so the caller can see exactly what was missing.
        let sheet = parse(":root { --a: var(--nope); --b: 1px; }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.value("--a"), Some("var(--nope)"));
        assert_eq!(vars.value("--b"), Some("1px"));
        assert_eq!(vars.unresolved(), &["--nope".to_owned()]);
    }

    #[test]
    fn detects_reference_cycles() {
        let sheet = parse(":root { --a: var(--b); --b: var(--a); }");
        let err = Variables::resolve(&sheet, None).unwrap_err();
        assert!(matches!(err, ResolveError::Cycle(_)));
    }

    #[test]
    fn later_declaration_wins() {
        let sheet = parse(":root { --a: 1px; } :root { --a: 2px; }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.value("--a"), Some("2px"));
    }

    #[test]
    fn important_beats_later_declaration() {
        let sheet = parse(":root { --a: 1px !important; } :root { --a: 2px; }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.value("--a"), Some("1px"));
    }

    #[test]
    fn theme_scope_isolation() {
        let sheet =
            parse(":root { --a: base; } .theme-dark { --a: dark; } .theme-light { --a: light; }");
        let dark = Variables::resolve(&sheet, Some(ThemeKind::Dark)).unwrap();
        assert_eq!(dark.value("--a"), Some("dark"));
        let light = Variables::resolve(&sheet, Some(ThemeKind::Light)).unwrap();
        assert_eq!(light.value("--a"), Some("light"));
    }

    #[test]
    fn root_fallback_applies_when_theme_does_not_override() {
        let sheet = parse(":root { --shared: 1px; } .theme-dark { --only-dark: 2px; }");
        let dark = Variables::resolve(&sheet, Some(ThemeKind::Dark)).unwrap();
        assert_eq!(dark.value("--shared"), Some("1px"));
        assert_eq!(dark.value("--only-dark"), Some("2px"));
    }

    #[test]
    fn hsl_companion_lookup() {
        let sheet = parse(":root { --brand-experiment: 1; --brand-experiment-hsl: 220 70% 60%; }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.hsl_of("--brand-experiment"), Some("220 70% 60%"));
        assert_eq!(vars.hsl_of("--missing"), None);
    }

    #[test]
    fn reports_unresolved_references_without_failing() {
        // Themes commonly rely on variables injected by the host client. That must not be fatal.
        let sheet = parse(":root { --a: var(--vc-injected); --b: 1px; }");
        let vars = Variables::resolve(&sheet, None).unwrap();
        assert_eq!(vars.unresolved(), &["--vc-injected".to_owned()]);
        assert_eq!(vars.value("--b"), Some("1px"));
    }

    #[test]
    fn translates_hashed_class_selectors() {
        let map = ClassMap::from_entries(
            std::collections::BTreeMap::from([
                (
                    "channel-2f1c9d".to_owned(),
                    ClassEntry {
                        stable: "channel".to_owned(),
                        surface: Surface::ChannelList,
                        notes: None,
                    },
                ),
                (
                    "channel-9zz".to_owned(),
                    ClassEntry {
                        stable: "channel".to_owned(),
                        surface: Surface::ChannelList,
                        notes: None,
                    },
                ),
            ]),
            None,
        )
        .unwrap();

        assert_eq!(translate_selector(".channel-2f1c9d", &map), ".channel");
        assert_eq!(
            translate_selector(".channel-2f1c9d.active", &map),
            ".channel.active"
        );
        assert_eq!(
            translate_selector(".channel-list > .channel-2f1c9d", &map),
            ".channel-list > .channel"
        );
    }

    #[test]
    fn translates_attribute_prefix_selectors() {
        // The legacy BetterDiscord idiom, ~1560 uses in the Silverfox corpus.
        let map = ClassMap::from_entries(
            std::collections::BTreeMap::from([(
                "messageContent-1c07e6".to_owned(),
                ClassEntry {
                    stable: "messageContent".to_owned(),
                    surface: Surface::Chat,
                    notes: None,
                },
            )]),
            None,
        )
        .unwrap();

        assert_eq!(
            translate_selector(r#"[class*="messageContent-1c07e6"]"#, &map),
            r#"[class*="messageContent"]"#
        );
        assert_eq!(
            translate_selector("[class^='messageContent-1c07e6']", &map),
            "[class^='messageContent']"
        );
    }

    #[test]
    fn leaves_unmapped_selectors_untouched() {
        let map = ClassMap::from_entries(
            std::collections::BTreeMap::from([(
                "known-1a".to_owned(),
                ClassEntry {
                    stable: "known".to_owned(),
                    surface: Surface::Unknown,
                    notes: None,
                },
            )]),
            None,
        )
        .unwrap();
        let original = ".unknown-2b > [data-is-self]";
        assert_eq!(translate_selector(original, &map), original);
    }

    #[test]
    fn translation_prefers_longest_match() {
        // A short mapping must not partially rewrite a longer hashed name.
        let map = ClassMap::from_entries(
            std::collections::BTreeMap::from([
                (
                    "channel-2f".to_owned(),
                    ClassEntry {
                        stable: "short".to_owned(),
                        surface: Surface::Unknown,
                        notes: None,
                    },
                ),
                (
                    "channel-2f1c9d".to_owned(),
                    ClassEntry {
                        stable: "long".to_owned(),
                        surface: Surface::ChannelList,
                        notes: None,
                    },
                ),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(translate_selector(".channel-2f1c9d", &map), ".long");
    }
}
