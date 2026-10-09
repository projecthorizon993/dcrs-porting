//! Package assembly: producing a `.serein-extension` file Serein will accept.
//!
//! The manifest is deliberately boring. Themes declare no capabilities and no actions — Serein's
//! host rejects a theme manifest that lists either — so there is no consent surface to get wrong
//! here, which is worth preserving rather than adding to.
//!
//! The shape mirrors `crates/extensions/src/lib.rs` in ViceVerse-cz/Serein exactly, including that
//! `deny_unknown_fields` is set on every level: a field the host does not know about fails the
//! whole install, so emitting a plausible-but-unknown key would produce a package that silently
//! never loads.

use std::collections::BTreeMap;

use dcrs_theme::color;
use serde::{Deserialize, Serialize};

use crate::map::Conversion;
use crate::theme::{Theme, ThemeError};

/// Manifest API version. Serein rejects anything other than its own.
pub const API_VERSION: u32 = 1;

/// Largest serialized package the host will parse, including both embedded byte arrays.
pub const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;

/// Largest embedded background or cover image.
pub const MAX_BACKGROUND_BYTES: usize = 2 * 1024 * 1024;

/// Largest width or height the host's decoder will accept.
pub const MAX_IMAGE_EDGE: u32 = 4096;

/// Largest decoded pixel count.
pub const MAX_IMAGE_PIXELS: u64 = 4_000_000;

/// Largest decoder allocation.
pub const MAX_IMAGE_ALLOC: u64 = 32 * 1024 * 1024;

/// What kind of package this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A declarative theme. The only kind this crate emits.
    Theme,
}

/// A package's manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version. Must equal [`API_VERSION`].
    pub api_version: u32,
    /// Stable id: lowercase ASCII, digits and dashes, at most 64 characters.
    pub id: String,
    /// Display name, 1–128 characters.
    pub name: String,
    /// Theme version, 1–128 characters.
    pub version: String,
    /// Who made it, 1–128 characters.
    pub author: String,
    /// SPDX-ish licence string, 1–128 characters.
    pub license: String,
    /// Source repository. May be empty for a theme, but must be a credential-free HTTPS URL if set.
    pub source: String,
    /// Package kind.
    pub kind: Kind,
    /// Always empty for a theme; the host rejects a theme that requests capabilities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    /// Always empty for a theme; the host rejects a theme that declares actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// A `.serein-extension` package.
///
/// Field order here is the host's declaration order, which is also the order a reader expects:
/// images first, then the manifest, then the theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// The single embedded conversation background, as raw PNG or JPEG bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub background_image: Vec<u8>,
    /// The theme card image for Settings > Themes, same constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cover_image: Vec<u8>,
    /// Identity and permissions.
    pub manifest: Manifest,
    /// The theme itself. Required for a `theme` package.
    pub theme: Theme,
    /// Always empty: a theme package has no Wasm module, and the host rejects one that does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wasm: Vec<u8>,
}

/// A package plus its serialized form.
///
/// The two travel together because the byte budget can only be checked by serializing, and a theme
/// with an embedded image is expensive to serialize.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The assembled package.
    pub package: Package,
    /// The exact bytes to write, already within [`MAX_PACKAGE_BYTES`].
    pub json: String,
}

/// Identity supplied by the caller.
///
/// Every field is non-optional because the host's manifest validation rejects an empty name,
/// version, author or licence. Defaults are filled in rather than left blank so a converter can
/// always emit an installable package, but `license` in particular is worth setting deliberately:
/// it is a legal claim about someone else's theme.
///
/// The description is not a manifest field — Serein's schema has no room for it — so it is carried
/// here only to be reported back to the user, not serialized.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    /// Theme id. Normalized to lowercase ASCII, matching Serein's own rule.
    pub id: String,
    /// Display name, at most 128 characters.
    pub name: String,
    /// Who made it. The host requires a non-empty value.
    pub author: String,
    /// Licence. The host requires a non-empty value.
    pub license: String,
    /// Theme version. The host requires a non-empty value.
    pub version: String,
    /// Source repository. Empty is allowed for a theme; a value must be a credential-free HTTPS URL.
    pub source: String,
    /// Description, for reporting only.
    pub description: Option<String>,
}

impl Identity {
    /// Builds an identity from a theme name, deriving the id by slugging it.
    ///
    /// The author defaults to `Unknown`, which is honest about not knowing rather than inventing a
    /// name; licence defaults to `All rights reserved` for the same reason.
    #[must_use]
    pub fn from_name(name: impl Into<String>, author: Option<String>) -> Self {
        let name = name.into();
        let id = slugify(&name);
        Self {
            id,
            name,
            author: author.unwrap_or_else(|| "Unknown".to_owned()),
            license: "All rights reserved".to_owned(),
            version: "1.0.0".to_owned(),
            source: String::new(),
            description: None,
        }
    }

    /// Fills in whatever a scraped [`Header`] knows and this identity does not.
    ///
    /// Header values win only where the identity has nothing to say, so an explicit `--author` flag
    /// still overrides what the theme claims about itself.
    #[must_use]
    pub fn with_header(mut self, header: Header) -> Self {
        if let Some(version) = header.version {
            self.version = version;
        }
        if let Some(license) = header.license {
            self.license = license;
        }
        if let Some(source) = header.source {
            self.source = source;
        }
        if let Some(description) = header.description {
            self.description = Some(description);
        }
        self
    }
}

/// Lowercases and replaces anything that is not an ASCII letter, digit or dash with a dash.
///
/// Serein normalizes imported ids to lowercase ASCII the same way, so an imported
/// `Golden-Theme` and a generated `golden-theme` are the same identity — which is what makes a
/// local edit replace the installed package rather than sitting beside it.
#[must_use]
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "theme".to_owned()
    } else if trimmed.len() > MAX_ID_LEN {
        trimmed[..MAX_ID_LEN].trim_end_matches('-').to_owned()
    } else {
        trimmed
    }
}

/// The host's id length limit.
const MAX_ID_LEN: usize = 64;

/// Device names Windows refuses to use as a file name.
///
/// The host rejects an id matching one of these, and it is easier to append a suffix than to hand
/// the user an install failure for a theme called "CON".
const RESERVED_IDS: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether an id satisfies the host's rules: lowercase ASCII, digits and dashes, 1–64 characters,
/// starting with an alphanumeric, and not a reserved device name.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && !RESERVED_IDS.contains(&id)
}

/// Slugs an id, guaranteeing the host will accept the result.
///
/// [`slugify`] alone can still produce a reserved name, so this is what the builder actually uses.
#[must_use]
pub fn safe_id(name: &str) -> String {
    let mut id = slugify(name);
    if RESERVED_IDS.contains(&id.as_str()) {
        id.push_str("-theme");
    }
    id
}

/// Errors from building a package.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    /// The theme failed Serein's own validation.
    #[error("theme would be rejected by the host: {0:?}")]
    InvalidTheme(Vec<ThemeError>),
    /// The conversion produced nothing at all.
    #[error("conversion produced no tokens or metrics")]
    Empty,
    /// Serialization failed.
    #[error("could not serialize package: {0}")]
    Serialize(#[from] serde_json::Error),
    /// A manifest field the host requires is missing or too long.
    #[error("manifest field {field:?} is invalid: {reason}")]
    Manifest {
        /// Offending field.
        field: &'static str,
        /// Why the host would reject it.
        reason: String,
    },
    /// An embedded image exceeds the host's limits.
    #[error("background image is {size} bytes; the host allows at most {limit}")]
    ImageTooLarge {
        /// Actual size.
        size: usize,
        /// Host limit.
        limit: usize,
    },
    /// The serialized package exceeds the host's limit.
    #[error("serialized package is {size} bytes; the host allows at most {limit}")]
    PackageTooLarge {
        /// Actual size.
        size: usize,
        /// Host limit.
        limit: usize,
    },
}

impl PackageError {
    /// Checks one manifest field the way the host does: non-empty, at most 128 characters, and no
    /// control characters.
    fn check_field(field: &'static str, value: &str) -> Result<(), PackageError> {
        if value.is_empty() {
            return Err(PackageError::Manifest {
                field,
                reason: "the host rejects an empty value".to_owned(),
            });
        }
        if value.len() > MAX_MANIFEST_FIELD_LEN {
            return Err(PackageError::Manifest {
                field,
                reason: format!(
                    "{} characters exceeds the host limit of {MAX_MANIFEST_FIELD_LEN}",
                    value.len()
                ),
            });
        }
        if value.chars().any(char::is_control) {
            return Err(PackageError::Manifest {
                field,
                reason: "contains a control character".to_owned(),
            });
        }
        Ok(())
    }
}

/// The host's per-field length limit for `name`, `version`, `author` and `license`.
const MAX_MANIFEST_FIELD_LEN: usize = 128;

/// Whether a `source` value is acceptable.
///
/// A theme may leave this empty; anything else must be a credential-free HTTPS URL, which is what
/// the host checks.
#[must_use]
pub fn valid_source(source: &str) -> bool {
    if source.is_empty() {
        return true;
    }
    source.len() <= 2048
        && source.starts_with("https://")
        && source.len() > "https://".len()
        && !source.contains('@')
        && !source.chars().any(char::is_whitespace)
        && !source.contains(char::is_control)
}

impl Package {
    /// Assembles a package from a conversion.
    ///
    /// The theme is validated *before* the package is built, because Serein rejects an invalid one
    /// at install time. Failing here means the user gets an actionable error instead of a package
    /// that silently refuses to load. The same applies to the manifest: the host validates it
    /// independently, and an empty `author` is the kind of mistake that costs a user an install.
    ///
    /// # Errors
    /// Returns [`PackageError::InvalidTheme`] if the theme violates Serein's schema,
    /// [`PackageError::Empty`] if nothing was mapped at all, [`PackageError::Manifest`] if a
    /// required field is missing or oversized, and [`PackageError::ImageTooLarge`] or
    /// [`PackageError::PackageTooLarge`] if an embedded image or the result blows a host limit.
    pub fn build(identity: Identity, conversion: &Conversion) -> Result<Self, PackageError> {
        Self::build_serialized(identity, conversion, Vec::new()).map(|built| built.package)
    }

    /// Assembles a package and serializes it once.
    ///
    /// The serialized bytes are returned alongside the package because the size check has to produce
    /// them anyway, and the caller needs them to write the file. Computing the length and then
    /// serializing a second time would build the whole document twice — and since a theme embeds its
    /// background as a JSON array of bytes, pretty-printing a 2 MiB image is on the order of 15 MB of
    /// `String` per pass.
    ///
    /// # Errors
    /// As [`Package::build`], plus [`PackageError::ImageTooLarge`] when the image exceeds 2 MiB and
    /// [`PackageError::PackageTooLarge`] when the serialized package exceeds 16 MiB.
    pub fn build_serialized(
        identity: Identity,
        conversion: &Conversion,
        background_image: Vec<u8>,
    ) -> Result<Built, PackageError> {
        let package = Self::assemble(identity, conversion, background_image)?;
        let json = package.to_json()?;
        if json.len() > MAX_PACKAGE_BYTES {
            return Err(PackageError::PackageTooLarge {
                size: json.len(),
                limit: MAX_PACKAGE_BYTES,
            });
        }
        Ok(Built { package, json })
    }

    /// Assembles a package carrying a conversation background.
    ///
    /// The image is raw PNG or JPEG bytes, never a URL: the host has no network access for themes,
    /// so a theme that wants an image has to carry it.
    ///
    /// # Errors
    /// As [`Package::build`], plus [`PackageError::ImageTooLarge`] when the image exceeds 2 MiB and
    /// [`PackageError::PackageTooLarge`] when the serialized package exceeds 16 MiB.
    pub fn build_with_image(
        identity: Identity,
        conversion: &Conversion,
        background_image: Vec<u8>,
    ) -> Result<Self, PackageError> {
        Self::build_serialized(identity, conversion, background_image).map(|built| built.package)
    }

    /// Validates identity and theme, then assembles. No serialization.
    fn assemble(
        identity: Identity,
        conversion: &Conversion,
        background_image: Vec<u8>,
    ) -> Result<Self, PackageError> {
        if background_image.len() > MAX_BACKGROUND_BYTES {
            return Err(PackageError::ImageTooLarge {
                size: background_image.len(),
                limit: MAX_BACKGROUND_BYTES,
            });
        }

        let theme = conversion.theme.clone();
        if let Err(errors) = theme.validate() {
            return Err(PackageError::InvalidTheme(errors));
        }
        if theme.is_empty() {
            return Err(PackageError::Empty);
        }

        let id = safe_id(&identity.id);
        if !valid_id(&id) {
            return Err(PackageError::Manifest {
                field: "id",
                reason: format!("{id:?} is not a valid package id"),
            });
        }
        PackageError::check_field("name", &identity.name)?;
        PackageError::check_field("version", &identity.version)?;
        PackageError::check_field("author", &identity.author)?;
        PackageError::check_field("license", &identity.license)?;
        if !valid_source(&identity.source) {
            return Err(PackageError::Manifest {
                field: "source",
                reason: "must be empty or a credential-free HTTPS URL".to_owned(),
            });
        }

        let package = Self {
            background_image,
            cover_image: Vec::new(),
            manifest: Manifest {
                api_version: API_VERSION,
                id,
                name: identity.name,
                version: identity.version,
                author: identity.author,
                license: identity.license,
                source: identity.source,
                kind: Kind::Theme,
                // A theme grants nothing. The host rejects a theme manifest that lists either, and
                // emitting an empty array would suggest the fields are part of the deal.
                capabilities: Vec::new(),
                actions: Vec::new(),
            },
            theme,
            wasm: Vec::new(),
        };

        Ok(package)
    }

    /// Serializes to pretty JSON, which is what the import flow expects to be readable.
    ///
    /// # Errors
    /// Returns [`serde_json::Error`] if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// How many colour tokens the package sets.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.theme.token_count()
    }
}

/// Resolves the CSS variables a theme declares under a given scope, for [`Conversion`] to consume.
///
/// `:root` applies to *both* appearances: it is the base Discord's own theme inherits from, and
/// skipping it for one appearance would silently halve a light-only theme. A theme-specific block is
/// layered on top, so a later declaration overrides the shared one, matching CSS.
#[must_use]
pub fn scope_vars(
    stylesheet: &dcrs_theme::Stylesheet,
    theme: dcrs_theme::ThemeKind,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();

    // One pass. `Rule::scopes` allocates, and the previous two-loop form walked every rule twice and
    // allocated twice per rule — multiplied by the two appearances a conversion resolves.
    for rule in stylesheet.rules() {
        let scopes = rule.scopes();
        // A rule carrying both `:root` and the theme class contributes to both; a rule carrying
        // neither is not a theme block and is skipped entirely.
        let applies = scopes.contains(&dcrs_theme::css::Scope::Root)
            || scopes
                .iter()
                .any(|s| matches!(*s, dcrs_theme::css::Scope::Theme(k) if k == theme));
        if !applies {
            continue;
        }
        for decl in &rule.declarations {
            if !decl.property.starts_with("--") {
                continue;
            }
            out.insert(decl.property.clone(), decl.value.clone());
        }
    }

    // Resolve `var()` against everything gathered, so a scope can reference a root value.
    if let Ok(resolved) = dcrs_theme::css::Variables::resolve(stylesheet, Some(theme)) {
        for (name, var) in resolved.iter() {
            // Only substitute values that actually evaluate as colours or metrics; anything else
            // (an unresolved reference, an unsupported function) keeps its raw form so the
            // conversion can report it rather than silently emit nonsense.
            let value = var.value.as_str();
            if color::evaluate(value).is_ok() || looks_numeric(value) {
                out.insert(name.clone(), value.to_owned());
            }
        }
    }

    out
}

/// Theme metadata scraped from `@`-prefixed comments in the CSS source.
///
/// BetterDiscord and Vencord both write identity as comments at the top of the file, which is the
/// only place a theme records an author or a licence. Reading them is what lets a converted package
/// carry real attribution instead of "Unknown".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// From `@name`.
    pub name: Option<String>,
    /// From `@author`.
    pub author: Option<String>,
    /// From `@description`.
    pub description: Option<String>,
    /// From `@version`.
    pub version: Option<String>,
    /// From `@license` or `@licence`.
    pub license: Option<String>,
    /// From `@source` or `@url`.
    pub source: Option<String>,
}

/// Reads the `@key value` header comments at the top of a stylesheet.
///
/// Themes write these several ways: one `@name Foo` per line, several on one line separated by
/// bullets, or inside a JSDoc-style block where every line starts with `*`. All are accepted, because
/// losing a theme's real name over formatting is exactly the failure that looks like a converter bug.
///
/// The scan stops at the first line of actual CSS. Comments further down are documentation rather than
/// metadata, and a theme is free to mention `@author` in prose.
#[must_use]
pub fn header_metadata(source: &str) -> Option<Header> {
    let mut header = Header::default();
    let mut found = false;
    let mut in_comment = false;

    for line in source.lines() {
        let mut rest = line.trim();

        if in_comment {
            let Some(end) = rest.find("*/") else {
                found |= scan_pairs(rest, &mut header);
                continue;
            };
            found |= scan_pairs(&rest[..end], &mut header);
            rest = rest[end + 2..].trim();
            in_comment = false;
        }

        if let Some(body) = rest.strip_prefix("/*") {
            if let Some(end) = body.find("*/") {
                found |= scan_pairs(&body[..end], &mut header);
                rest = body[end + 2..].trim();
            } else {
                in_comment = true;
                found |= scan_pairs(body, &mut header);
                continue;
            }
        }

        // Strip the leading `*` a JSDoc block puts on every continuation line.
        let content = rest.trim_start_matches(['*', ' ', '\t']).trim_start();

        if content.is_empty() {
            continue;
        }
        if starts_css(content) {
            break;
        }
        found |= scan_pairs(content, &mut header);
    }

    found.then_some(header)
}

/// Whether a line is the start of real CSS rather than header prose.
fn starts_css(content: &str) -> bool {
    if content.starts_with([':', '.', '#', '{', '}']) {
        return true;
    }
    // An at-rule ends the header; `@name` does not, because `name` is not an at-rule.
    if let Some(rest) = content.strip_prefix('@') {
        let key: String = rest
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_lowercase();
        return matches!(
            key.as_str(),
            "media" | "import" | "supports" | "font-face" | "keyframes"
        );
    }
    false
}

/// Finds every `@key value` in one comment line.
///
/// A value runs to the next separator — a bullet, a star, or the end of the line — rather than to the
/// next space, because names routinely contain spaces.
fn scan_pairs(text: &str, header: &mut Header) -> bool {
    let mut found = false;
    let mut rest = text;

    while let Some(at) = rest.find('@') {
        let after = &rest[at + 1..];
        let key_len = after
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(after.len());
        let key = &after[..key_len];
        let value_part = after[key_len..].trim_start();
        // A value runs to the next field. Separators vary by author — bullets, stars, pipes — and the
        // most reliable one is the next `@key`, which is why it is checked alongside the rest.
        let value: String = value_part
            .chars()
            .take_while(|c| !matches!(c, '•' | '·' | '*' | '|' | '\n' | '@'))
            .collect();
        let value = value.trim().to_owned();
        found |= record(header, key, &value);
        rest = &after[key_len + value_part.len() - value_part.trim_end().len()..];
        if key.is_empty() {
            rest = &rest[1..];
        }
    }
    found
}

/// Stores one header key, returning whether it was recognized.
fn record(header: &mut Header, key: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let value = value.to_owned();
    match key.trim_start_matches('@').to_ascii_lowercase().as_str() {
        "name" => header.name = Some(value),
        "author" | "authors" => header.author = Some(value),
        "description" => header.description = Some(value),
        "version" => header.version = Some(value),
        "license" | "licence" => header.license = Some(value),
        "source" | "url" | "website" | "homepage" => header.source = Some(value),
        _ => return false,
    }
    true
}

/// Whether a value is a plain number, length or percentage.
fn looks_numeric(value: &str) -> bool {
    let t = value.trim();
    t.ends_with('%') || t.ends_with("px") || t.ends_with("rem") || t.parse::<f32>().is_ok()
}

/// Converts a parsed theme into a package, reporting what did not survive.
///
/// The dropped list is the union of both passes, so the report names every variable that failed to
/// project in either mode rather than just the last one tried.
#[must_use]
pub fn from_theme(identity: Identity, stylesheet: &dcrs_theme::Stylesheet) -> PackageBuild {
    from_theme_with_image(identity, stylesheet, Vec::new())
}

/// Converts a parsed theme into a package carrying a conversation background.
///
/// The bytes are the decoded image itself. Serein has no network access for themes, so a theme that
/// wants an image must carry it, and the caller is the only thing that can resolve a URL.
#[must_use]
pub fn from_theme_with_image(
    identity: Identity,
    stylesheet: &dcrs_theme::Stylesheet,
    background_image: Vec<u8>,
) -> PackageBuild {
    let conversion = convert(stylesheet);
    let build = Package::build_with_image(identity, &conversion, background_image);
    PackageBuild {
        conversion,
        package: build,
    }
}

/// Projects both appearances out of a stylesheet.
///
/// Both are resolved against the shared `:root` block, because a Discord theme's `:root`
/// declarations are the base that `.theme-dark` and `.theme-light` both inherit. A theme that only
/// ever writes `:root` therefore lands on both, which is the common case.
#[must_use]
pub fn convert(stylesheet: &dcrs_theme::Stylesheet) -> Conversion {
    let dark = scope_vars(stylesheet, dcrs_theme::ThemeKind::Dark);
    let light = scope_vars(stylesheet, dcrs_theme::ThemeKind::Light);

    let mut conversion = crate::map::convert(&dark, &dark);
    if light != dark {
        let light_conversion = crate::map::convert(&light, &light);
        conversion.theme.light = light_conversion.theme.light;
        conversion.dropped.extend(light_conversion.dropped);
    }
    conversion.tokens_mapped = conversion.theme.token_count();
    conversion.variables_seen = dark.len().max(light.len());
    conversion
}

/// The conversion plus whatever package came out of it.
#[derive(Debug)]
pub struct PackageBuild {
    /// The conversion, for reporting.
    pub conversion: Conversion,
    /// The package, or why there is not one.
    pub package: Result<Package, PackageError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Token;

    fn conversion_from(pairs: &[(&str, &str)]) -> Conversion {
        let dark = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        crate::map::convert(&BTreeMap::new(), &dark)
    }

    #[test]
    fn slugify_matches_sereins_normalization() {
        assert_eq!(slugify("Golden Theme"), "golden-theme");
        assert_eq!(slugify("ClearVision v7"), "clearvision-v7");
        assert_eq!(slugify("Midnight///Dawn"), "midnight-dawn");
        assert_eq!(slugify("  spaced  out  "), "spaced-out");
        // Non-ASCII is dropped rather than kept, since Serein restricts ids to ASCII.
        assert_eq!(slugify("Théme Ok"), "th-me-ok");
        assert_eq!(slugify("!!!"), "theme", "must never produce an empty id");
    }

    #[test]
    fn identity_derives_an_id_from_the_name() {
        let id = Identity::from_name("My Cool Theme", Some("me".to_owned()));
        assert_eq!(id.id, "my-cool-theme");
        assert_eq!(id.name, "My Cool Theme");
        assert_eq!(id.author, "me");
        // The host rejects an empty version, author or licence, so these must never be blank.
        assert!(!id.version.is_empty(), "version must be filled in");
        assert!(!id.license.is_empty(), "licence must be filled in");
        assert!(id.source.is_empty(), "a theme may leave source empty");
    }

    #[test]
    fn a_theme_package_grants_no_capabilities() {
        let conversion =
            conversion_from(&[("--text-normal", "#ffffff"), ("--text-muted", "#888888")]);
        let package = Package::build(Identity::from_name("T", None), &conversion).unwrap();
        assert_eq!(package.manifest.api_version, API_VERSION);
        assert_eq!(package.manifest.kind, Kind::Theme);
        assert!(
            package.wasm.is_empty(),
            "a theme package carries no Wasm module"
        );
        assert!(
            package.manifest.capabilities.is_empty(),
            "a theme must not request capabilities"
        );
        assert!(
            package.manifest.actions.is_empty(),
            "a theme declares no actions"
        );
        // `text`, `text_strong` and `muted`.
        assert_eq!(package.token_count(), 3);
    }

    #[test]
    fn package_serializes_to_readable_json() {
        let conversion = conversion_from(&[("--brand-experiment", "#5865f2")]);
        let package = Package::build(
            Identity::from_name("Accent Only", Some("me".into())),
            &conversion,
        )
        .unwrap();
        let json = package.to_json().unwrap();
        assert!(json.contains("\"kind\": \"theme\""), "{json}");
        assert!(json.contains("\"manifest\""), "{json}");
        assert!(json.contains("\"api_version\": 1"), "{json}");
        assert!(json.contains("\"accent\": \"#5865f2\""), "{json}");
        assert!(
            json.contains("\"accent_text\""),
            "accent text should be derived: {json}"
        );
        assert!(json.contains('\n'), "output should be pretty-printed");
    }

    #[test]
    fn a_reserved_device_name_is_escaped() {
        assert_eq!(safe_id("CON"), "con-theme");
        assert_eq!(safe_id("lpt1"), "lpt1-theme");
        assert_eq!(
            safe_id("con2"),
            "con2",
            "only exact reserved names are escaped"
        );
        assert!(valid_id(&safe_id("CON")));
    }

    #[test]
    fn source_must_be_empty_or_https() {
        assert!(valid_source(""));
        assert!(valid_source("https://github.com/someone/theme"));
        assert!(
            !valid_source("http://github.com/someone/theme"),
            "plain HTTP is refused"
        );
        assert!(
            !valid_source("https://user:pw@example.com"),
            "credentials are refused"
        );
        assert!(!valid_source("https://"), "a host is required");
    }

    #[test]
    fn a_manifest_field_the_host_rejects_is_reported() {
        let conversion = conversion_from(&[("--text-normal", "#ffffff")]);
        let mut identity = Identity::from_name("Fine", None);
        identity.author = String::new();
        let err = Package::build(identity, &conversion).unwrap_err();
        assert!(
            matches!(
                err,
                PackageError::Manifest {
                    field: "author",
                    ..
                }
            ),
            "{err:?}"
        );

        let mut long = Identity::from_name("Fine", None);
        long.name = "x".repeat(129);
        let err = Package::build(long, &conversion).unwrap_err();
        assert!(
            matches!(err, PackageError::Manifest { field: "name", .. }),
            "{err:?}"
        );
    }

    #[test]
    fn an_oversized_background_is_refused() {
        let conversion = conversion_from(&[("--text-normal", "#ffffff")]);
        let image = vec![0u8; MAX_BACKGROUND_BYTES + 1];
        let err = Package::build_with_image(Identity::from_name("Big", None), &conversion, image)
            .unwrap_err();
        assert!(matches!(err, PackageError::ImageTooLarge { .. }), "{err:?}");
    }

    #[test]
    fn an_empty_conversion_is_rejected_rather_than_emitted() {
        let conversion = crate::map::convert(&BTreeMap::new(), &BTreeMap::new());
        assert!(matches!(
            Package::build(Identity::from_name("Nothing", None), &conversion),
            Err(PackageError::Empty)
        ));
    }

    #[test]
    fn an_invalid_theme_is_rejected_at_build_time() {
        // Force an out-of-range metric past `validate` by mutating the converted theme.
        let mut conversion = conversion_from(&[("--text-normal", "#ffffff")]);
        conversion.theme.style.body_size = Some(200);
        let err = Package::build(Identity::from_name("Bad", None), &conversion).unwrap_err();
        assert!(matches!(err, PackageError::InvalidTheme(_)), "{err:?}");
    }

    #[test]
    fn scope_vars_prefers_theme_over_root() {
        let sheet = dcrs_theme::Stylesheet::parse(
            ":root { --a: #111111; --b: #222222; } .theme-dark { --a: #333333; }",
        )
        .unwrap();
        let vars = scope_vars(&sheet, dcrs_theme::ThemeKind::Dark);
        // The theme-scoped declaration wins; the root-only one still applies.
        assert_eq!(vars.get("--a").map(String::as_str), Some("#333333"));
        assert_eq!(vars.get("--b").map(String::as_str), Some("#222222"));
    }

    #[test]
    fn scope_vars_resolves_var_references() {
        let sheet = dcrs_theme::Stylesheet::parse(
            ":root { --brand-experiment: #5865f2; } .theme-dark { --text-link: var(--brand-experiment); }",
        )
        .unwrap();
        let vars = scope_vars(&sheet, dcrs_theme::ThemeKind::Dark);
        assert_eq!(vars.get("--text-link").map(String::as_str), Some("#5865f2"));
    }

    #[test]
    fn from_theme_produces_a_package_end_to_end() {
        let sheet = dcrs_theme::Stylesheet::parse(
            "\
:root { --background-primary: #1e1f22; }
.theme-dark {
    --background-secondary: #2b2d31;
    --chat-background: #111214;
    --text-normal: #dbdee1;
    --text-muted: #949ba4;
    --brand-experiment: #5865f2;
    --font-size-md: 16px;
}",
        )
        .unwrap();
        let build = from_theme(
            Identity::from_name("Ported", Some("someone".into())),
            &sheet,
        );
        let package = build.package.expect("should build");
        assert_eq!(package.theme.dark.colors.get(Token::Text), Some("#dbdee1"));
        assert_eq!(
            package.theme.dark.colors.get(Token::Accent),
            Some("#5865f2")
        );
        assert_eq!(package.theme.style.body_size, Some(16));
        assert!(package.token_count() >= 5, "got {}", package.token_count());
        assert!(package.theme.validate().is_ok());
    }

    #[test]
    fn looks_numeric_recognises_lengths_and_percentages() {
        assert!(looks_numeric("16px"));
        assert!(looks_numeric("1.5rem"));
        assert!(looks_numeric("80%"));
        assert!(looks_numeric("32"));
        assert!(!looks_numeric("#ffffff"));
        assert!(!looks_numeric("hsl(235 86% 66%)"));
    }

    #[test]
    fn a_one_line_header_is_read() {
        // The common BetterDiscord format: everything on one line, bullets between fields.
        let header =
            header_metadata("/* @name Example Theme • @author dcrs • @version 1.0 */\n:root {}")
                .expect("header should be found");
        assert_eq!(header.name.as_deref(), Some("Example Theme"));
        assert_eq!(header.author.as_deref(), Some("dcrs"));
        assert_eq!(header.version.as_deref(), Some("1.0"));
    }

    #[test]
    fn a_jsdoc_header_is_read() {
        let source = "\
/*
 * @name ClearVision
 * @description A theme
 * @author someone
 * @version 7.0
 * @license MIT
 * @source https://github.com/someone/ClearVision
 */
:root { --a: #111; }";
        let header = header_metadata(source).expect("header should be found");
        assert_eq!(header.name.as_deref(), Some("ClearVision"));
        assert_eq!(header.description.as_deref(), Some("A theme"));
        assert_eq!(header.author.as_deref(), Some("someone"));
        assert_eq!(header.version.as_deref(), Some("7.0"));
        assert_eq!(header.license.as_deref(), Some("MIT"));
        assert_eq!(
            header.source.as_deref(),
            Some("https://github.com/someone/ClearVision")
        );
    }

    #[test]
    fn the_header_scan_stops_at_the_first_css() {
        // `@author` mentioned in a comment mid-theme is prose, not metadata.
        let source = ":root { --a: #111; }\n/* @author not the author */";
        assert!(header_metadata(source).is_none());
    }

    #[test]
    fn an_at_rule_ends_the_header_but_a_bullet_does_not() {
        let source = "/* @name T */\n@media (min-width: 100px) { :root { --a: #111; } }";
        let header = header_metadata(source).expect("header should be found");
        assert_eq!(header.name.as_deref(), Some("T"));
    }

    #[test]
    fn no_header_means_none() {
        assert!(header_metadata(":root { --a: #111; }").is_none());
        assert!(header_metadata("").is_none());
    }

    #[test]
    fn a_header_fills_only_what_the_identity_lacks() {
        let header = Header {
            version: Some("2.0".to_owned()),
            license: Some("MIT".to_owned()),
            source: Some("https://example.com".to_owned()),
            description: Some("d".to_owned()),
            ..Header::default()
        };
        let identity = Identity::from_name("T", Some("me".to_owned())).with_header(header);
        assert_eq!(identity.version, "2.0");
        assert_eq!(identity.license, "MIT");
        assert_eq!(identity.source, "https://example.com");
        assert_eq!(
            identity.author, "me",
            "an explicit author is not overwritten"
        );
    }

    #[test]
    fn a_light_palette_replaces_the_shared_one() {
        let sheet = dcrs_theme::Stylesheet::parse(
            ":root { --background-primary: #111111; }
.theme-dark { --background-primary: #000000; }
.theme-light { --background-primary: #ffffff; }",
        )
        .unwrap();
        let conversion = convert(&sheet);
        assert_eq!(
            conversion.theme.dark.colors.get(Token::Base),
            Some("#000000")
        );
        assert_eq!(
            conversion.theme.light.colors.get(Token::Base),
            Some("#ffffff")
        );
    }
}
