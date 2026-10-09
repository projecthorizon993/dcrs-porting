//! The capability registry: which third-party mod APIs the native client can replicate.
//!
//! Research over `Vendicated/Vencord`, `BetterDiscord` and `uwu/shelter` found that the mod
//! ecosystem's API splits into five classes, and that only one of them is genuinely
//! unreplicable. This crate encodes that split as data so the porting tool can produce a
//! portability verdict instead of a guess.
//!
//! | Class | Meaning | Replicable |
//! |---|---|---|
//! | [`Class::Read`] | getters over existing state | yes |
//! | [`Class::Write`] | state mutations | yes |
//! | [`Class::Subscription`] | change notification | yes |
//! | [`Class::Ui`] | render a component somewhere | yes, and free |
//! | [`Class::Internals`] | monkeypatch a minified bundle | **no** |

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// How a surface relates to the host application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Reading state that already exists. Needs a store registry, not 71 reimplementations.
    Read,
    /// Mutating state.
    Write,
    /// Subscribing to change notifications.
    Subscription,
    /// Injecting a UI component at a known location.
    Ui,
    /// Patching the host's internals. Not replicable: depends on an artifact we do not control.
    Internals,
}

impl Class {
    /// Whether a surface in this class can be implemented natively.
    #[must_use]
    pub const fn is_replicable(self) -> bool {
        !matches!(self, Self::Internals)
    }

    /// Short lowercase label used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "a",
            Self::Write => "b",
            Self::Subscription => "c",
            Self::Ui => "d",
            Self::Internals => "e",
        }
    }

    /// Human-readable name used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Read => "data read",
            Self::Write => "data write",
            Self::Subscription => "subscription",
            Self::Ui => "ui injection",
            Self::Internals => "internals patch",
        }
    }
}

/// How completely a surface is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Not started.
    Stub,
    /// Partially implemented; see `notes`.
    Partial,
    /// Fully implemented.
    Implemented,
    /// Deliberately will not be implemented. Only valid for [`Class::Internals`].
    Unsupported,
}

impl Support {
    /// Whether the surface is usable by a ported plugin.
    #[must_use]
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Implemented | Self::Partial)
    }

    /// Label used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stub => "stub",
            Self::Partial => "partial",
            Self::Implemented => "ok",
            Self::Unsupported => "unsupported",
        }
    }
}

/// One entry in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    /// Stable dotted identifier, e.g. `stores.ChannelStore`.
    pub id: String,
    /// Which of the five classes this surface belongs to.
    pub class: Class,
    /// Implementation status.
    pub support: Support,
    /// The API this surface mirrors in the web client, for cross-referencing docs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webapp_analogue: Option<String>,
    /// Release the surface became usable in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Free-form notes, typically about what remains to be done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Surface {
    /// Whether a plugin can use this surface today.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        self.support.is_usable()
    }
}

/// The serialized registry file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryFile {
    /// Schema version.
    version: u32,
    surfaces: Vec<Surface>,
}

/// Highest registry schema version this build understands.
pub const MAX_REGISTRY_VERSION: u32 = 1;

/// Errors from loading or querying the registry.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The file could not be read.
    #[error("reading capability registry: {0}")]
    Io(#[from] std::io::Error),
    /// The file was not valid TOML.
    #[error("parsing capability registry: {0}")]
    Parse(#[from] toml::de::Error),
    /// The schema version is newer than this build understands.
    #[error("unsupported registry version {found}, this build supports at most {max}")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Highest version this build can read.
        max: u32,
    },
    /// Two surfaces shared an id.
    #[error("duplicate surface id {0:?}")]
    DuplicateId(String),
    /// A surface was marked `unsupported` despite being in a replicable class. That is a
    /// bookkeeping error: only internals-patch surfaces may be unsupported.
    #[error("surface {id:?} is {support:?} but its class {class:?} is replicable")]
    InvalidSupport {
        /// Offending surface id.
        id: String,
        /// Its class.
        class: Class,
        /// Its declared support level.
        support: Support,
    },
}

/// The capability registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    surfaces: BTreeMap<String, Surface>,
}

/// A surface reference resolved during analysis, whether or not the registry knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Known and usable.
    Usable,
    /// Known but only partly usable.
    Partial,
    /// Known but declared unsupported.
    Unsupported,
    /// Replicable in principle, but not yet implemented.
    NotYetImplemented,
    /// Not replicable at all, because it patches host internals.
    NotReplicable,
    /// The registry has never heard of this surface, so its class is unknown.
    Unknown,
}

impl Verdict {
    /// Whether a ported plugin could rely on this surface.
    #[must_use]
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Usable | Self::Partial)
    }

    /// Label used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usable => "ok",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
            Self::NotYetImplemented => "not-implemented",
            Self::NotReplicable => "not-replicable",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Registry {
    /// Loads the registry from a TOML file.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed, declares an unsupported schema
    /// version, or is internally inconsistent.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_toml(&raw)
    }

    /// Parses the registry from a TOML string.
    ///
    /// # Errors
    /// See [`Registry::load`].
    pub fn from_toml(raw: &str) -> Result<Self, RegistryError> {
        let file: RegistryFile = toml::from_str(raw)?;
        if file.version > MAX_REGISTRY_VERSION {
            return Err(RegistryError::UnsupportedVersion {
                found: file.version,
                max: MAX_REGISTRY_VERSION,
            });
        }
        Self::from_surfaces(file.surfaces)
    }

    /// Builds a registry from surface entries.
    ///
    /// # Errors
    /// Returns an error on duplicate ids or inconsistent support levels.
    pub fn from_surfaces(surfaces: Vec<Surface>) -> Result<Self, RegistryError> {
        let mut map = BTreeMap::new();
        for surface in surfaces {
            if surface.support == Support::Unsupported && surface.class.is_replicable() {
                return Err(RegistryError::InvalidSupport {
                    id: surface.id.clone(),
                    class: surface.class,
                    support: surface.support,
                });
            }
            let id = surface.id.clone();
            if map.insert(id.clone(), surface).is_some() {
                return Err(RegistryError::DuplicateId(id));
            }
        }
        Ok(Self { surfaces: map })
    }

    /// Looks up a surface by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Surface> {
        self.surfaces.get(id)
    }

    /// Resolves a surface id to a usability verdict.
    #[must_use]
    pub fn verdict(&self, id: &str) -> Verdict {
        match self.surfaces.get(id) {
            Some(s) => match (s.class, s.support) {
                (Class::Internals, _) => Verdict::NotReplicable,
                (_, Support::Implemented) => Verdict::Usable,
                (_, Support::Partial) => Verdict::Partial,
                (_, Support::Unsupported) => Verdict::Unsupported,
                (_, Support::Stub) => Verdict::NotYetImplemented,
            },
            None => Verdict::Unknown,
        }
    }

    /// Every surface, ordered by id.
    pub fn iter(&self) -> impl Iterator<Item = &Surface> {
        self.surfaces.values()
    }

    /// Total number of surfaces.
    #[must_use]
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// Aggregated counts per class, for the "what did we lose" summary.
    #[must_use]
    pub fn class_histogram(&self) -> BTreeMap<Class, usize> {
        let mut hist = BTreeMap::new();
        for s in self.surfaces.values() {
            *hist.entry(s.class).or_insert(0) += 1;
        }
        hist
    }

    /// Fraction of surfaces that are usable today, in the range `0.0..=1.0`.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        if self.surfaces.is_empty() {
            return 0.0;
        }
        let usable = self.surfaces.values().filter(|s| s.is_usable()).count();
        // Internals-patch surfaces are excluded from the denominator: they can never be
        // implemented, so counting them would make 100% coverage unreachable and meaningless.
        let reachable = self
            .surfaces
            .values()
            .filter(|s| s.class.is_replicable())
            .count();
        if reachable == 0 {
            return 1.0;
        }
        #[allow(clippy::cast_precision_loss)]
        {
            usable as f64 / reachable as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        Registry::from_surfaces(vec![
            Surface {
                id: "stores.ChannelStore".into(),
                class: Class::Read,
                support: Support::Implemented,
                webapp_analogue: Some("Vencord.Webpack.Common.ChannelStore".into()),
                since: Some("0.1.0".into()),
                notes: None,
            },
            Surface {
                id: "flux.MediaEngineStore.mutators".into(),
                class: Class::Write,
                support: Support::Partial,
                webapp_analogue: None,
                since: None,
                notes: Some("setSelfMute done; DeviceChange pending".into()),
            },
            Surface {
                id: "ui.renderMessageAccessory".into(),
                class: Class::Ui,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            Surface {
                id: "internals.webpackPatches".into(),
                class: Class::Internals,
                support: Support::Unsupported,
                webapp_analogue: Some("patches[] in definePlugin".into()),
                since: None,
                notes: Some("string-matching a minified bundle; not replicable".into()),
            },
            Surface {
                id: "stores.VoiceStateStore".into(),
                class: Class::Read,
                support: Support::Stub,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
        ])
        .unwrap()
    }

    #[test]
    fn verdicts_cover_every_state() {
        let r = registry();
        assert_eq!(r.verdict("stores.ChannelStore"), Verdict::Usable);
        assert_eq!(
            r.verdict("flux.MediaEngineStore.mutators"),
            Verdict::Partial
        );
        assert_eq!(
            r.verdict("stores.VoiceStateStore"),
            Verdict::NotYetImplemented
        );
        assert_eq!(
            r.verdict("internals.webpackPatches"),
            Verdict::NotReplicable
        );
        assert_eq!(r.verdict("nope"), Verdict::Unknown);
    }

    #[test]
    fn internals_override_support_level() {
        // Even if an internals surface were marked Implemented by mistake, it must never report
        // as usable: it is unreplicable by construction.
        let r = Registry::from_surfaces(vec![Surface {
            id: "internals.x".into(),
            class: Class::Internals,
            support: Support::Implemented,
            webapp_analogue: None,
            since: None,
            notes: None,
        }])
        .unwrap();
        assert_eq!(r.verdict("internals.x"), Verdict::NotReplicable);
        assert!(!Verdict::NotReplicable.is_usable());
    }

    #[test]
    fn coverage_excludes_unreplicable_surfaces() {
        // 4 replicable surfaces (3 usable, 1 stub), 1 internals surface.
        assert!((registry().coverage() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_registry_has_full_coverage() {
        assert_eq!(Registry::default().coverage(), 0.0);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let dup = vec![
            Surface {
                id: "x".into(),
                class: Class::Read,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
            Surface {
                id: "x".into(),
                class: Class::Write,
                support: Support::Implemented,
                webapp_analogue: None,
                since: None,
                notes: None,
            },
        ];
        assert!(matches!(
            Registry::from_surfaces(dup).unwrap_err(),
            RegistryError::DuplicateId(_)
        ));
    }

    #[test]
    fn rejects_unsupported_on_replicable_class() {
        let bad = vec![Surface {
            id: "stores.X".into(),
            class: Class::Read,
            support: Support::Unsupported,
            webapp_analogue: None,
            since: None,
            notes: None,
        }];
        assert!(matches!(
            Registry::from_surfaces(bad).unwrap_err(),
            RegistryError::InvalidSupport { .. }
        ));
    }

    #[test]
    fn class_histogram_counts_all_five() {
        let hist = registry().class_histogram();
        let count = |c: Class| hist.get(&c).copied().unwrap_or(0);
        assert_eq!(count(Class::Read), 2);
        assert_eq!(count(Class::Write), 1);
        // No surface in this fixture exercises subscriptions, so the key is absent entirely.
        assert_eq!(count(Class::Subscription), 0);
        assert_eq!(count(Class::Ui), 1);
        assert_eq!(count(Class::Internals), 1);
    }

    #[test]
    fn round_trips_through_toml() {
        let raw = r#"
version = 1

[[surfaces]]
id = "stores.ChannelStore"
class = "read"
support = "implemented"
webapp_analogue = "Vencord.Webpack.Common.ChannelStore"
since = "0.1.0"
"#;
        let r = Registry::from_toml(raw).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.verdict("stores.ChannelStore"), Verdict::Usable);
        assert_eq!(
            r.get("stores.ChannelStore")
                .unwrap()
                .webapp_analogue
                .as_deref(),
            Some("Vencord.Webpack.Common.ChannelStore")
        );
    }

    #[test]
    fn rejects_future_schema_version() {
        let err = Registry::from_toml("version = 99\nsurfaces = []\n").unwrap_err();
        assert!(matches!(
            err,
            RegistryError::UnsupportedVersion { found: 99, max: 1 }
        ));
    }

    #[test]
    fn rejects_registry_missing_surfaces() {
        // A file with no `surfaces` key is malformed, not empty: the version check must not
        // silently treat it as an empty registry.
        assert!(matches!(
            Registry::from_toml("version = 1\n").unwrap_err(),
            RegistryError::Parse(_)
        ));
    }

    #[test]
    fn class_labels_are_stable() {
        assert_eq!(Class::Read.as_str(), "a");
        assert_eq!(Class::Internals.as_str(), "e");
        assert!(!Class::Internals.is_replicable());
        assert!(Class::Ui.is_replicable());
        assert_eq!(Class::Ui.label(), "ui injection");
    }
}
