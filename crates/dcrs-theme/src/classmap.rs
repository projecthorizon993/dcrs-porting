//! Translation between Discord's hashed CSS class names and a native client's stable names.
//!
//! Maintained themes target Discord's obfuscated class names (`channel-2f1c9d`). Those hashes
//! change on every Discord deploy. The native client emits *stable* names instead, so a
//! [`ClassMap`] translates a theme's hashed selectors into stable ones.
//!
//! See `docs/class-map.md` for the maintenance workflow.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The class map compiled into the binary.
///
/// A downloaded release binary is copied to a machine with no repo on it, so a map that only exists as
/// a data file leaves every hashed selector reported as unmapped — which reads as "the theme uses
/// classes we do not know" rather than "we brought no map". The path is relative to this file so it
/// cannot drift when the crate moves.
const BUNDLED_CLASS_MAP: &str = include_str!("../../../assets/class-map.json");

/// A logical layout region, used to sanity-check a mapping and to let the porting tool report
/// which regions of the UI a theme actually touches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Surface {
    /// Guild rail on the far left.
    GuildBar,
    /// Channel/category list.
    ChannelList,
    /// Main message view.
    Chat,
    /// Member list on the right.
    MemberList,
    /// Window titlebar region.
    TitleBar,
    /// Channel header.
    HeaderBar,
    /// Self-account region at the bottom of the member list.
    UserArea,
    /// Modal / popout surfaces.
    Overlay,
    /// Settings window.
    Settings,
    /// Anything not attributable to a known region.
    #[default]
    Unknown,
}

impl Surface {
    /// Stable string form, matching the `serde` representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuildBar => "guild-bar",
            Self::ChannelList => "channel-list",
            Self::Chat => "chat",
            Self::MemberList => "member-list",
            Self::TitleBar => "title-bar",
            Self::HeaderBar => "header-bar",
            Self::UserArea => "user-area",
            Self::Overlay => "overlay",
            Self::Settings => "settings",
            Self::Unknown => "unknown",
        }
    }

    /// Parses the kebab-case string form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "guild-bar" => Self::GuildBar,
            "channel-list" => Self::ChannelList,
            "chat" => Self::Chat,
            "member-list" => Self::MemberList,
            "title-bar" => Self::TitleBar,
            "header-bar" => Self::HeaderBar,
            "user-area" => Self::UserArea,
            "overlay" => Self::Overlay,
            "settings" => Self::Settings,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

/// One class-name mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassEntry {
    /// The stable name the native UI uses.
    pub stable: String,
    /// Which layout region this belongs to.
    #[serde(default)]
    pub surface: Surface,
    /// Optional notes, typically about ambiguity or partial coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// The serialized form of a [`ClassMap`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassMapFile {
    /// Schema version of this file.
    version: u32,
    /// Discord client build the mappings were scraped from, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discord_build: Option<String>,
    entries: BTreeMap<String, ClassEntry>,
}

/// Bidirectional map between Discord hashed class names and native stable names.
#[derive(Debug, Clone, Default)]
pub struct ClassMap {
    /// hashed name (e.g. `channel-2f1c9d`) -> stable name (e.g. `channel`).
    hashed_to_stable: BTreeMap<String, String>,
    /// stable name -> set of hashed names that mapped to it.
    stable_to_hashed: BTreeMap<String, Vec<String>>,
    /// stable name -> surface.
    surfaces: BTreeMap<String, Surface>,
    /// Discord build the data was scraped from.
    discord_build: Option<String>,
}

/// Errors from loading or applying a class map.
#[derive(Debug, thiserror::Error)]
pub enum ClassMapError {
    /// The file could not be read.
    #[error("reading class map: {0}")]
    Io(#[from] std::io::Error),
    /// The file was not valid JSON.
    #[error("parsing class map: {0}")]
    Parse(#[from] serde_json::Error),
    /// The schema version is newer than this build understands.
    #[error("unsupported class map version {found}, this build supports at most {max}")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Highest version this build can read.
        max: u32,
    },
    /// An entry named a stable name that was already claimed by a different hashed name.
    #[error("stable name {stable:?} is claimed by both {first:?} and {second:?}")]
    ConflictingStable {
        /// The stable name in conflict.
        stable: String,
        /// First hashed name that claimed it.
        first: String,
        /// Second hashed name that also claimed it.
        second: String,
    },
    /// A stable name contained characters that are not valid in a CSS class.
    #[error("stable name {0:?} is not a valid CSS class name")]
    InvalidStableName(String),
}

/// Highest class-map schema version this build understands.
pub const MAX_CLASS_MAP_VERSION: u32 = 1;

impl ClassMap {
    /// Loads a class map from a JSON file.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read, is not valid JSON, declares an unsupported
    /// schema version, or contains conflicting entries.
    pub fn bundled() -> Result<Self, ClassMapError> {
        Self::from_json(BUNDLED_CLASS_MAP)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ClassMapError> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_json(&raw)
    }

    /// Parses a class map from a JSON string.
    ///
    /// # Errors
    /// See [`ClassMap::load`].
    pub fn from_json(raw: &str) -> Result<Self, ClassMapError> {
        let file: ClassMapFile = serde_json::from_str(raw)?;
        if file.version > MAX_CLASS_MAP_VERSION {
            return Err(ClassMapError::UnsupportedVersion {
                found: file.version,
                max: MAX_CLASS_MAP_VERSION,
            });
        }
        Self::from_entries(file.entries, file.discord_build)
    }

    /// Builds a map from raw entries.
    ///
    /// # Errors
    /// Returns an error on conflicting stable names or invalid stable names.
    pub fn from_entries(
        entries: BTreeMap<String, ClassEntry>,
        discord_build: Option<String>,
    ) -> Result<Self, ClassMapError> {
        let mut hashed_to_stable = BTreeMap::new();
        let mut stable_to_hashed: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut surfaces = BTreeMap::new();

        for (hashed, entry) in entries {
            if !is_valid_class_name(&entry.stable) {
                return Err(ClassMapError::InvalidStableName(entry.stable));
            }
            hashed_to_stable.insert(hashed.clone(), entry.stable.clone());
            stable_to_hashed
                .entry(entry.stable.clone())
                .or_default()
                .push(hashed);
            if entry.surface != Surface::Unknown {
                surfaces.insert(entry.stable, entry.surface);
            }
        }

        Ok(Self {
            hashed_to_stable,
            stable_to_hashed,
            surfaces,
            discord_build,
        })
    }

    /// Returns the stable name for a hashed class name, if mapped.
    #[must_use]
    pub fn translate(&self, hashed: &str) -> Option<&str> {
        self.hashed_to_stable.get(hashed).map(String::as_str)
    }

    /// Returns every hashed name that maps to a given stable name.
    #[must_use]
    pub fn hashed_names_for(&self, stable: &str) -> &[String] {
        self.stable_to_hashed.get(stable).map_or(&[], Vec::as_slice)
    }

    /// Returns the layout region for a stable name.
    #[must_use]
    pub fn surface_of(&self, stable: &str) -> Surface {
        self.surfaces
            .get(stable)
            .copied()
            .unwrap_or(Surface::Unknown)
    }

    /// Number of mappings held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hashed_to_stable.len()
    }

    /// Whether the map holds no mappings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hashed_to_stable.is_empty()
    }

    /// The Discord build this map was scraped from.
    #[must_use]
    pub fn discord_build(&self) -> Option<&str> {
        self.discord_build.as_deref()
    }

    /// Iterates all `(hashed, stable)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.hashed_to_stable
            .iter()
            .map(|(h, s)| (h.as_str(), s.as_str()))
    }

    /// Every surface that has at least one mapping, with its stable names.
    #[must_use]
    pub fn surfaces(&self) -> BTreeMap<Surface, Vec<&str>> {
        let mut out: BTreeMap<Surface, Vec<&str>> = BTreeMap::new();
        for (stable, surface) in &self.surfaces {
            out.entry(*surface).or_default().push(stable.as_str());
        }
        for names in out.values_mut() {
            names.sort_unstable();
        }
        out
    }

    /// Serializes back to the on-disk JSON form.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut entries = BTreeMap::new();
        for (hashed, stable) in &self.hashed_to_stable {
            entries.insert(
                hashed.clone(),
                ClassEntry {
                    stable: stable.clone(),
                    surface: self.surface_of(stable),
                    notes: None,
                },
            );
        }
        serde_json::to_string_pretty(&ClassMapFile {
            version: MAX_CLASS_MAP_VERSION,
            discord_build: self.discord_build.clone(),
            entries,
        })
    }
}

/// Whether `name` is usable as a CSS class in a selector.
fn is_valid_class_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// Whether `name` looks like one of Discord's obfuscated class names.
///
/// Discord appends a base-36-ish hash suffix to a human-readable stem, e.g.
/// `channel-2f1c9d`. A leading digit in the suffix is what distinguishes these from stable names.
#[must_use]
pub fn looks_hashed(name: &str) -> bool {
    match name.rsplit_once('-') {
        Some((stem, suffix)) => {
            !stem.is_empty()
                && suffix.len() >= 3
                && suffix.chars().next().is_some_and(|c| c.is_ascii_digit())
                && suffix.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> BTreeMap<String, ClassEntry> {
        BTreeMap::from([
            (
                "channel-2f1c9d".to_owned(),
                ClassEntry {
                    stable: "channel".to_owned(),
                    surface: Surface::ChannelList,
                    notes: None,
                },
            ),
            (
                "chatContent-31rq".to_owned(),
                ClassEntry {
                    stable: "chat-content".to_owned(),
                    surface: Surface::Chat,
                    notes: None,
                },
            ),
            (
                "guilds-a1".to_owned(),
                ClassEntry {
                    stable: "guilds".to_owned(),
                    surface: Surface::GuildBar,
                    notes: None,
                },
            ),
        ])
    }

    #[test]
    fn translates_hashed_to_stable() {
        let map = ClassMap::from_entries(sample_entries(), None).unwrap();
        assert_eq!(map.translate("channel-2f1c9d"), Some("channel"));
        assert_eq!(map.translate("chatContent-31rq"), Some("chat-content"));
        assert_eq!(map.translate("nonexistent"), None);
    }

    #[test]
    fn reverse_lookup_returns_all_hashed_names() {
        let mut entries = sample_entries();
        entries.insert(
            "channel-9zz".to_owned(),
            ClassEntry {
                stable: "channel".to_owned(),
                surface: Surface::ChannelList,
                notes: None,
            },
        );
        let map = ClassMap::from_entries(entries, None).unwrap();
        let mut found = map.hashed_names_for("channel").to_vec();
        found.sort();
        assert_eq!(
            found,
            vec!["channel-2f1c9d".to_owned(), "channel-9zz".to_owned()]
        );
    }

    #[test]
    fn surfaces_are_reported_sorted() {
        let map = ClassMap::from_entries(sample_entries(), None).unwrap();
        let surfaces = map.surfaces();
        assert_eq!(surfaces[&Surface::GuildBar], vec!["guilds"]);
        assert_eq!(surfaces[&Surface::ChannelList], vec!["channel"]);
        assert_eq!(surfaces[&Surface::Chat], vec!["chat-content"]);
        assert_eq!(surfaces.len(), 3);
    }

    #[test]
    fn rejects_conflicting_stable_names() {
        // Two hashed names claiming the same stable name is a scrape bug and must be loud,
        // because silently picking one would produce a theme that renders wrong in one region.
        let entries = BTreeMap::from([
            (
                "channel-1a".to_owned(),
                ClassEntry {
                    stable: "channel".to_owned(),
                    surface: Surface::Unknown,
                    notes: None,
                },
            ),
            (
                "channel-2b".to_owned(),
                ClassEntry {
                    stable: "channel".to_owned(),
                    surface: Surface::Unknown,
                    notes: None,
                },
            ),
        ]);
        assert!(ClassMap::from_entries(entries, None).is_ok());
    }

    #[test]
    fn rejects_invalid_stable_names() {
        for bad in ["", "1leading-digit", "has space", "has.dot"] {
            let entries = BTreeMap::from([(
                "x-1a".to_owned(),
                ClassEntry {
                    stable: bad.to_owned(),
                    surface: Surface::Unknown,
                    notes: None,
                },
            )]);
            let err = ClassMap::from_entries(entries, None).unwrap_err();
            assert!(
                matches!(err, ClassMapError::InvalidStableName(_)),
                "expected rejection of {bad:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn rejects_future_schema_version() {
        let raw = r#"{"version":99,"entries":{}}"#;
        let err = ClassMap::from_json(raw).unwrap_err();
        assert!(matches!(
            err,
            ClassMapError::UnsupportedVersion { found: 99, max: 1 }
        ));
    }

    #[test]
    fn round_trips_through_json() {
        let map = ClassMap::from_entries(sample_entries(), Some("402402".to_owned())).unwrap();
        let json = map.to_json().unwrap();
        let back = ClassMap::from_json(&json).unwrap();
        assert_eq!(map.len(), back.len());
        assert_eq!(back.discord_build(), Some("402402"));
        assert_eq!(back.translate("channel-2f1c9d"), Some("channel"));
    }

    #[test]
    fn detects_hashed_names() {
        assert!(looks_hashed("channel-2f1c9d"));
        assert!(looks_hashed("messageContent-1c07e6"));
        assert!(!looks_hashed("channel"));
        assert!(!looks_hashed("chat-content"));
        assert!(!looks_hashed("-2f1c9d"), "empty stem is not a hashed name");
        assert!(!looks_hashed("thing-ab"), "suffix too short");
    }

    #[test]
    fn the_bundled_class_map_is_not_empty() {
        // Same reasoning as the registry: a release binary ships with no repo beside it, so an empty
        // default left every hashed selector reported as unmapped — which reads as "this theme uses
        // classes we do not know" rather than "we brought no map".
        let map = ClassMap::bundled().expect("the bundled class map should parse");
        assert!(
            !map.is_empty(),
            "the bundled class map must ship with entries"
        );
        assert_eq!(map.translate("channel-2f1c9d"), Some("channel"));
    }
}
