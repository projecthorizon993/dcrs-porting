//! Plugin manifest, mirroring Vencord's `PluginDef` field-for-field.

use serde::{Deserialize, Serialize};

/// A plugin author. Discord ids are snowflakes and exceed 53-bit float precision in JS, which is
/// why Vencord uses `BigInt`; here a `u64` is the natural type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    /// Display name.
    pub name: String,
    /// Discord user id.
    pub id: u64,
}

impl Author {
    /// Builds an author entry.
    #[must_use]
    pub fn new(name: impl Into<String>, id: u64) -> Self {
        Self {
            name: name.into(),
            id,
        }
    }
}

/// Vencord's 21 `PluginTag` values, used for search and categorisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PluginTag {
    Accessibility,
    Activity,
    Appearance,
    Chat,
    Commands,
    Console,
    Customisation,
    Developers,
    Emotes,
    Friends,
    Fun,
    Media,
    Notifications,
    Organisation,
    Privacy,
    Reactions,
    Roles,
    Servers,
    Shortcuts,
    Utility,
    Voice,
}

impl PluginTag {
    /// Every tag, for iteration and validation.
    pub const ALL: [Self; 21] = [
        Self::Accessibility,
        Self::Activity,
        Self::Appearance,
        Self::Chat,
        Self::Commands,
        Self::Console,
        Self::Customisation,
        Self::Developers,
        Self::Emotes,
        Self::Friends,
        Self::Fun,
        Self::Media,
        Self::Notifications,
        Self::Organisation,
        Self::Privacy,
        Self::Reactions,
        Self::Roles,
        Self::Servers,
        Self::Shortcuts,
        Self::Utility,
        Self::Voice,
    ];

    /// Tag name as it appears in source plugins.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accessibility => "Accessibility",
            Self::Activity => "Activity",
            Self::Appearance => "Appearance",
            Self::Chat => "Chat",
            Self::Commands => "Commands",
            Self::Console => "Console",
            Self::Customisation => "Customisation",
            Self::Developers => "Developers",
            Self::Emotes => "Emotes",
            Self::Friends => "Friends",
            Self::Fun => "Fun",
            Self::Media => "Media",
            Self::Notifications => "Notifications",
            Self::Organisation => "Organisation",
            Self::Privacy => "Privacy",
            Self::Reactions => "Reactions",
            Self::Roles => "Roles",
            Self::Servers => "Servers",
            Self::Shortcuts => "Shortcuts",
            Self::Utility => "Utility",
            Self::Voice => "Voice",
        }
    }
}

impl std::fmt::Display for PluginTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When a plugin should start, mirroring Vencord's `StartAt`.
///
/// `WebpackReady` has no analogue in a native client, so it is accepted and treated as
/// [`StartAt::Init`] with a note in the porting report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartAt {
    /// As early as possible.
    #[default]
    Init,
    /// Once the UI tree exists.
    DomContentLoaded,
    /// Once every module is available. No native analogue; treated as `Init`.
    WebpackReady,
}

/// Where a plugin's UI is injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderSlot {
    /// Beside each message.
    MessageAccessory,
    /// Above/below each message.
    MessageDecoration,
    /// In the member list beside a member.
    MemberListDecorator,
    /// In the chat composer toolbar.
    ChatBarButton,
    /// In a message's hover popover.
    MessagePopoverButton,
    /// On a user's profile.
    ProfileBadge,
    /// In the settings sidebar header.
    SettingsAbout,
    /// In the guild-rail toolbox menu.
    Toolbox,
    /// Contributed to a context menu, keyed by menu id.
    ContextMenu,
    /// In the settings UI, as a whole panel.
    SettingsPanel,
}

/// The kind of a settings option, mirroring Vencord's `OptionType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OptionKind {
    /// Free text.
    String,
    /// Numeric.
    Number,
    /// Snowflake.
    Bigint,
    /// Toggle.
    Boolean,
    /// Fixed set of choices.
    Select,
    /// Bounded numeric.
    Slider,
    /// A rendered widget. Not supported natively; requires a plugin-provided renderer.
    Component,
    /// Anything else.
    Custom,
}

impl OptionKind {
    /// Whether the host can render this option without plugin help.
    #[must_use]
    pub const fn is_host_renderable(self) -> bool {
        !matches!(self, Self::Component | Self::Custom)
    }
}

/// A plugin's static description.
///
/// The boolean flags mirror Vencord's `PluginDef` one-for-one, so a ported manifest stays
/// recognizable against its source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Manifest {
    /// Unique identifier. Must match `^[a-z0-9-]+$`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Authors.
    #[serde(default)]
    pub authors: Vec<Author>,
    /// Extra search terms for the settings search box.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_terms: Vec<String>,
    /// Category tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<PluginTag>,
    /// Ids of plugins that must be enabled first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Cannot be disabled by the user.
    #[serde(default)]
    pub required: bool,
    /// Hidden from the plugin list.
    #[serde(default)]
    pub hidden: bool,
    /// Enabled on first install.
    #[serde(default)]
    pub enabled_by_default: bool,
    /// Needs a restart to take effect.
    #[serde(default)]
    pub requires_restart: bool,
    /// Start phase.
    #[serde(default)]
    pub start_at: StartAt,
    /// UI slots this plugin contributes to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub render_slots: Vec<RenderSlot>,
    /// Settings keys and their types.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<OptionSpec>,
    /// Surface ids this plugin requires, checked against the capability registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_surfaces: Vec<String>,
}

/// One declared settings option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    /// Settings key, namespaced under the plugin id.
    pub key: String,
    /// Value type.
    pub kind: OptionKind,
    /// Label shown in the settings UI.
    pub display_name: Option<String>,
    /// Longer explanation.
    pub description: Option<String>,
    /// Choice labels, when `kind` is `Select`.
    pub choices: Vec<String>,
    /// Changing this needs a restart.
    #[serde(default)]
    pub restart_needed: bool,
}

impl OptionSpec {
    /// A minimal boolean option.
    #[must_use]
    pub fn boolean(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            kind: OptionKind::Boolean,
            display_name: None,
            description: None,
            choices: Vec::new(),
            restart_needed: false,
        }
    }
}

/// Manifest validation failures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    /// The id was not lowercase alphanumeric-with-dashes.
    #[error("plugin id {0:?} must match ^[a-z0-9-]+$")]
    InvalidId(String),
    /// The name was empty or absurdly long.
    #[error("plugin name must be 1..=128 characters, got {0}")]
    InvalidName(usize),
    /// The description was empty or absurdly long.
    #[error("plugin description must be 1..=512 characters, got {0}")]
    InvalidDescription(usize),
    /// The plugin declared a dependency on itself.
    #[error("plugin {0:?} depends on itself")]
    SelfDependency(String),
}

impl Manifest {
    /// Validates the manifest.
    ///
    /// # Errors
    /// Returns an error if the id is malformed, the name or description length is out of range,
    /// or the plugin depends on itself.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(ManifestError::InvalidId(self.id.clone()));
        }
        if self.name.is_empty() || self.name.len() > 128 {
            return Err(ManifestError::InvalidName(self.name.len()));
        }
        if self.description.is_empty() || self.description.len() > 512 {
            return Err(ManifestError::InvalidDescription(self.description.len()));
        }
        if self.dependencies.iter().any(|d| d == &self.id) {
            return Err(ManifestError::SelfDependency(self.id.clone()));
        }
        Ok(())
    }

    /// Whether this manifest needs a UI contribution the host cannot render alone.
    #[must_use]
    pub fn needs_plugin_rendered_options(&self) -> bool {
        self.options.iter().any(|o| !o.kind.is_host_renderable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            id: "better-nitro".to_owned(),
            name: "FakeNitro".to_owned(),
            description: "Pretends you have Nitro".to_owned(),
            authors: vec![Author::new("someone", 1_234_567_890)],
            search_terms: vec!["premium".to_owned()],
            tags: vec![PluginTag::Fun, PluginTag::Customisation],
            dependencies: vec![],
            required: false,
            hidden: false,
            enabled_by_default: false,
            requires_restart: true,
            start_at: StartAt::WebpackReady,
            render_slots: vec![RenderSlot::MessageDecoration],
            options: vec![OptionSpec::boolean("fakeIt")],
            required_surfaces: vec!["stores.ChannelStore".to_owned()],
        }
    }

    #[test]
    fn valid_manifest_passes() {
        manifest().validate().unwrap();
    }

    #[test]
    fn rejects_bad_ids() {
        for bad in ["FakeNitro", "fake nitro", "fake_nitro", "", "fake.nitro"] {
            let mut m = manifest();
            m.id = bad.to_owned();
            assert!(
                matches!(m.validate(), Err(ManifestError::InvalidId(_))),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_self_dependency() {
        let mut m = manifest();
        m.dependencies = vec![m.id.clone()];
        assert!(matches!(
            m.validate(),
            Err(ManifestError::SelfDependency(_))
        ));
    }

    #[test]
    fn rejects_out_of_range_lengths() {
        let mut m = manifest();
        m.name = String::new();
        assert!(matches!(m.validate(), Err(ManifestError::InvalidName(0))));
        m.name = "x".repeat(129);
        assert!(matches!(m.validate(), Err(ManifestError::InvalidName(129))));
        m.name = "ok".to_owned();
        m.description = String::new();
        assert!(matches!(
            m.validate(),
            Err(ManifestError::InvalidDescription(0))
        ));
    }

    #[test]
    fn tag_set_is_complete_and_unique() {
        let names: Vec<&str> = PluginTag::ALL.iter().map(|t| t.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), 21, "expected 21 tags, got {names:?}");
        assert_eq!(names.len(), sorted.len(), "duplicate tag names: {names:?}");
    }

    #[test]
    fn component_options_are_not_host_renderable() {
        assert!(OptionKind::Boolean.is_host_renderable());
        assert!(OptionKind::Slider.is_host_renderable());
        assert!(!OptionKind::Component.is_host_renderable());
        assert!(!OptionKind::Custom.is_host_renderable());
    }

    #[test]
    fn detects_options_needing_plugin_renderer() {
        let mut m = manifest();
        assert!(!m.needs_plugin_rendered_options());
        m.options.push(OptionSpec {
            key: "custom".to_owned(),
            kind: OptionKind::Component,
            display_name: None,
            description: None,
            choices: vec![],
            restart_needed: false,
        });
        assert!(m.needs_plugin_rendered_options());
    }

    #[test]
    fn round_trips_through_json() {
        let m = manifest();
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, m.id);
        assert_eq!(back.tags, m.tags);
        assert_eq!(back.start_at, StartAt::WebpackReady);
        back.validate().unwrap();
    }
}
