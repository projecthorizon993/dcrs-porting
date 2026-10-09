//! Converting a theme to a Serein package, and reporting what did not survive.
//!
//! The interesting output is not the package — it is the list of declarations the host has no place
//! for. A converted theme always looks like it worked, because Serein accepts any subset of its
//! tokens, so without an explicit account of what was dropped a silently-worse theme is
//! indistinguishable from a faithful port.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use dcrs_serein::{Conversion, Dropped, Identity};

/// Identity fields supplied on the command line.
///
/// Grouped so [`ConversionReport::apply`] takes one argument instead of five, and so adding a flag
/// later does not touch the call site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// Theme name.
    pub name: Option<String>,
    /// Author.
    pub author: Option<String>,
    /// Licence.
    pub license: Option<String>,
    /// Theme version.
    pub version: Option<String>,
    /// Source repository URL.
    pub source: Option<String>,
}

/// A conversion plus the report meant to be read alongside the package.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversionReport {
    /// Theme name, from the `@name` header or the file stem.
    pub name: String,
    /// Author, from the `@author` header.
    pub author: Option<String>,
    /// Licence, from the `@license` header.
    pub license: Option<String>,
    /// Theme version, from the `@version` header.
    pub version: Option<String>,
    /// Source URL, from the `@source` header.
    pub source: Option<String>,
    /// The conversion itself.
    pub conversion: Conversion,
    /// Why there is no package, when there is not one.
    pub failure: Option<String>,
}

impl ConversionReport {
    /// Builds a package from this report, serialized once.
    ///
    /// Rebuilding on demand rather than holding one alongside the report is deliberate: a converter
    /// that produced a report and a separate package risks the two disagreeing about what happened.
    ///
    /// # Errors
    /// Propagates whatever the host would reject: an invalid theme, a bad manifest field, or an
    /// oversized image.
    pub fn package(
        &self,
        background_image: Vec<u8>,
    ) -> Result<dcrs_serein::Built, dcrs_serein::PackageError> {
        dcrs_serein::Package::build_serialized(self.identity(), &self.conversion, background_image)
    }

    /// The identity a package built from this report would carry.
    #[must_use]
    pub fn identity(&self) -> Identity {
        let mut identity = Identity::from_name(self.name.clone(), self.author.clone());
        if let Some(license) = self.license.clone() {
            identity.license = license;
        }
        if let Some(version) = self.version.clone() {
            identity.version = version;
        }
        if let Some(source) = self.source.clone() {
            identity.source = source;
        }
        identity
    }

    /// Overrides identity fields from explicit command-line flags.
    ///
    /// Flags beat the theme's own headers: someone naming a theme on the command line is stating an
    /// intent, not restating a guess.
    pub fn apply(&mut self, overrides: Overrides) {
        if let Some(name) = overrides.name {
            self.name = name;
        }
        if let Some(author) = overrides.author {
            self.author = Some(author);
        }
        if let Some(license) = overrides.license {
            self.license = Some(license);
        }
        if let Some(version) = overrides.version {
            self.version = Some(version);
        }
        if let Some(source) = overrides.source {
            self.source = Some(source);
        }
    }

    /// How many Serein colour tokens were filled, across both appearances.
    #[must_use]
    pub fn tokens_mapped(&self) -> usize {
        self.conversion.tokens_mapped
    }

    /// How many declarations could not be projected onto anything the host reads.
    #[must_use]
    pub fn dropped_count(&self) -> usize {
        self.conversion.dropped.len()
    }

    /// Whether anything was lost.
    ///
    /// Not an error: most themes lose declarations, because a theme is free to set things Serein has
    /// no concept of. But a report with nothing in it is the claim that the port is complete.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.conversion.dropped.is_empty()
    }

    /// Declarations grouped by which Serein target they were aiming at.
    ///
    /// Grouping matters more than listing here. `style.body_size` failing tells you the theme wanted
    /// a different body size; `dark.colors.link` tells you the same. A flat list of 200 dropped
    /// variables is the shape a report takes when it is not trying to say anything.
    pub fn dropped_by_area(&self) -> Vec<(&'static str, Vec<&Dropped>)> {
        let mut areas: BTreeMap<&'static str, Vec<&Dropped>> = BTreeMap::new();
        for item in &self.conversion.dropped {
            let area = if item.target.starts_with("style.") {
                "control metrics"
            } else {
                "colors"
            };
            areas.entry(area).or_default().push(item);
        }
        areas.into_iter().collect()
    }

    /// Rendered as plain text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "theme: {}", self.name);
        if let Some(author) = &self.author {
            let _ = writeln!(out, "  author: {author}");
        }
        let total = self.conversion.theme.token_count();
        let _ = writeln!(
            out,
            "  tokens filled {}/36 ({:.0}%)",
            total,
            self.conversion.coverage() * 100.0
        );
        if let Some(failure) = &self.failure {
            let _ = writeln!(out, "  FAILED: {failure}");
        }
        if self.conversion.dropped.is_empty() {
            let _ = writeln!(out, "  dropped: nothing");
        } else {
            let _ = writeln!(out, "  dropped {} declaration(s)", self.dropped_count());
            for (area, items) in self.dropped_by_area() {
                let _ = writeln!(out, "    {area}:");
                for item in items {
                    let _ = writeln!(out, "      {} <- {}", item.target, item.variable);
                    let _ = writeln!(out, "        {}", item.reason);
                }
            }
        }
        out
    }
}

/// Converts theme CSS into a conversion report.
///
/// The theme name comes from an `@name` header when the source has one, because that is where
/// BetterDiscord and Vencord put it and it is what the author called the thing.
///
/// # Errors
/// Returns an error if the CSS cannot be parsed at all. A theme with unparseable CSS is not a theme
/// that converts badly, so this is the one case worth failing loudly on.
pub fn convert(
    source: &str,
    fallback_name: &str,
) -> Result<ConversionReport, dcrs_theme::ParseError> {
    let sheet = dcrs_theme::Stylesheet::parse(source)?;
    Ok(convert_parsed(source, &sheet, fallback_name))
}

/// Converts an already-parsed stylesheet.
///
/// Taking the [`dcrs_theme::Stylesheet`] rather than the source text is what keeps a conversion to one
/// parse. Parsing twice — once to convert, once to look for a background image — rebuilt hundreds of
/// rules and their declarations and held them alive at the same time, roughly doubling peak memory
/// for a 50 KB input.
pub fn convert_parsed(
    source: &str,
    sheet: &dcrs_theme::Stylesheet,
    fallback_name: &str,
) -> ConversionReport {
    let header = dcrs_serein::package::header_metadata(source).unwrap_or_default();

    let name = header.name.unwrap_or_else(|| fallback_name.to_owned());
    let conversion = dcrs_serein::convert(sheet);

    ConversionReport {
        name,
        author: header.author,
        license: header.license,
        version: header.version,
        source: header.source,
        conversion,
        failure: None,
    }
}

/// Converts a theme and returns the report alongside any background image already in the source.
///
/// One parse for both, which is the whole point: a caller that wants a package with an embedded
/// image should not have to parse the file twice or hold two copies of its rules at once.
///
/// # Errors
/// Returns an error if the CSS cannot be parsed.
pub fn convert_file(
    source: &str,
    fallback_name: &str,
) -> Result<(ConversionReport, Option<Vec<u8>>), dcrs_theme::ParseError> {
    let sheet = dcrs_theme::Stylesheet::parse(source)?;
    let background = embedded_background_in(&sheet);
    Ok((convert_parsed(source, &sheet, fallback_name), background))
}

/// Reads an embedded background image out of theme CSS.
///
/// Serein will not fetch an image URL for a theme, so a theme that wants a background has to carry
/// the bytes. Discord themes set theirs in `body { background-image: url(...) }`, sometimes inside a
/// `background` shorthand and sometimes with a data URL inline.
///
/// Only `data:` URLs are resolved here, because they are already in the file; a relative or remote URL
/// needs a network fetch, which is the caller's decision, not this function's.
///
/// # Errors
/// Returns the parse error if the CSS cannot be read.
#[must_use]
pub fn embedded_background(source: &str) -> Option<Vec<u8>> {
    let sheet = dcrs_theme::Stylesheet::parse(source).ok()?;
    embedded_background_in(&sheet)
}

/// The image search over an already-parsed stylesheet.
fn embedded_background_in(sheet: &dcrs_theme::Stylesheet) -> Option<Vec<u8>> {
    for rule in sheet.rules() {
        for decl in &rule.declarations {
            // Covers `background-image` and the `background` shorthand; a `background-color` cannot
            // hold a URL and is filtered out by the parser rather than tested here.
            if !decl.property.starts_with("background") {
                continue;
            }
            if let Some(bytes) = data_url_bytes(&decl.value) {
                return Some(bytes);
            }
        }
    }
    None
}

/// Decodes the payload of a `data:` URL, when it looks like a PNG or JPEG.
///
/// Restricting to those two formats is not a limitation but a requirement: the host's decoder accepts
/// only PNG and JPEG, and rejects anything else at install time.
fn data_url_bytes(value: &str) -> Option<Vec<u8>> {
    let inner = value.trim().strip_prefix("url(")?.strip_suffix(')')?;
    // `url("data:...")` is legal CSS and themes in the wild use both forms.
    let inner = inner.trim().trim_matches(['"', '\'']);
    let rest = inner.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.contains("image/png") && !meta.contains("image/jpeg") {
        return None;
    }
    if meta.contains("base64") {
        base64_decode(payload.trim())
    } else {
        // A percent-encoded data URL is legal but vanishingly rare for images, and handling it would
        // mean another decoder for no realistic theme.
        None
    }
}

/// Decodes standard base64, ignoring whitespace.
///
/// Hand-rolled because the alternative is a dependency for about thirty lines, and because the only
/// payloads that reach here are theme images the caller already owns.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        let Some(value) = base64_value(byte) else {
            if byte.is_ascii_whitespace() || byte == b'=' {
                continue;
            }
            return None;
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }
    Some(out)
}

/// The six-bit value of a base64 character, or `None` if it is not one.
fn base64_value(byte: u8) -> Option<u8> {
    Some(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

/// The limits the host enforces, for reporting before an install fails.
///
/// Worth printing because every one of them is a hard rejection at install time rather than a
/// runtime fallback.
#[must_use]
pub fn host_limits() -> Vec<(&'static str, String)> {
    vec![
        ("package bytes", dcrs_serein::MAX_PACKAGE_BYTES.to_string()),
        (
            "background bytes",
            dcrs_serein::MAX_BACKGROUND_BYTES.to_string(),
        ),
        ("image edge px", dcrs_serein::MAX_IMAGE_EDGE.to_string()),
        ("image pixels", dcrs_serein::MAX_IMAGE_PIXELS.to_string()),
        (
            "decoder alloc MiB",
            (dcrs_serein::MAX_IMAGE_ALLOC / (1024 * 1024)).to_string(),
        ),
    ]
}
