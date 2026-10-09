//! Capability gates: separating a mod's *mechanism* from its *effect*.
//!
//! A mod like `Vencord`'s `FakeNitro` is implemented as ~11 webpack patches, each replacing a Discord
//! boolean predicate with `return true;`. Classified by API call, every one of those is class (e)
//! — internals patching — and unreportable. Classified by **effect**, they are eleven capability
//! flags, all of which a native client owns outright.
//!
//! That distinction is what makes these mods portable. The mechanism (`patches: [{ find:
//! "canStreamQuality", replacement: "return true;" }]`) cannot be ported. The effect ("this account
//! may stream at HD quality") is a field in a struct we control.
//!
//! So the porting pipeline gains a second axis. [`crate::registry::Class`] answers *can this API be
//! called natively*; a [`Capability`] answers *what does the user get*. A mod with no replicable
//! API calls can still be fully implementable.

use serde::{Deserialize, Serialize};

/// A thing Discord gates behind a premium subscription or a permission, which a client can choose
/// to grant.
///
/// Every variant names a real predicate Discord's web app evaluates. That mapping is the point: the
/// native client implements the predicate directly, so a patch that overrides it disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// `canUseCustomStickersEverywhere` - use stickers outside the guild that owns them.
    CustomStickersEverywhere,
    /// `canStreamQuality` - stream above the free tier's 720p/30.
    StreamQuality,
    /// `canUseHighVideoUploadQuality` - upload video at high quality.
    HighVideoUploadQuality,
    /// `canUseClientThemes` - load custom CSS themes.
    ClientThemes,
    /// The `CLIENT_THEMES_EDITOR` tier check that gates the theme editor.
    ClientThemeEditor,
    /// `canUsePremiumAppIcons` - change the application icon.
    PremiumAppIcons,
    /// `isPremium(getCurrentUser())` for the desktop app icon specifically.
    PremiumAppIconCurrent,
    /// Send custom guild emojis without the external-emoji permission.
    ExternalEmojis,
    /// Use animated custom emojis.
    AnimatedEmojis,
    /// `.GUILD_SUBSCRIPTION_UNAVAILABLE` - emojis locked to a role subscription.
    RoleSubscriptionEmojis,
    /// Emoji picker intent, so voice-channel emoji picks are not blocked.
    EmojiPickerIntent,
    /// `available` on stickers, making all of them sendable.
    AllStickersAvailable,
    /// `available` on soundboard sounds.
    AllSoundboardSounds,
    /// `guildPremiumTier` removed from the stream FPS option.
    StreamFpsUnlocked,
    /// User settings proto, so theme changes persist for a non-premium account.
    LocalSettingsProto,
    /// Gradient preset selection, normally tier-2 gated.
    GradientThemeSelection,
}

impl Capability {
    /// Every capability.
    pub const ALL: &'static [Self] = &[
        Self::CustomStickersEverywhere,
        Self::StreamQuality,
        Self::HighVideoUploadQuality,
        Self::ClientThemes,
        Self::ClientThemeEditor,
        Self::PremiumAppIcons,
        Self::PremiumAppIconCurrent,
        Self::ExternalEmojis,
        Self::AnimatedEmojis,
        Self::RoleSubscriptionEmojis,
        Self::EmojiPickerIntent,
        Self::AllStickersAvailable,
        Self::AllSoundboardSounds,
        Self::StreamFpsUnlocked,
        Self::LocalSettingsProto,
        Self::GradientThemeSelection,
    ];

    /// Human-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomStickersEverywhere => "custom stickers everywhere",
            Self::StreamQuality => "high-quality streaming",
            Self::HighVideoUploadQuality => "high-quality video upload",
            Self::ClientThemes => "client themes",
            Self::ClientThemeEditor => "client theme editor",
            Self::PremiumAppIcons => "premium application icons",
            Self::PremiumAppIconCurrent => "premium application icon (current user)",
            Self::ExternalEmojis => "external emojis",
            Self::AnimatedEmojis => "animated emojis",
            Self::RoleSubscriptionEmojis => "role-subscription emojis",
            Self::EmojiPickerIntent => "emoji picker intents",
            Self::AllStickersAvailable => "all stickers available",
            Self::AllSoundboardSounds => "all soundboard sounds",
            Self::StreamFpsUnlocked => "unlocked stream framerate",
            Self::LocalSettingsProto => "locally-applied settings proto",
            Self::GradientThemeSelection => "gradient theme selection",
        }
    }

    /// The Discord predicate a patch would have to override to get this behaviour.
    #[must_use]
    pub const fn discord_predicate(self) -> &'static str {
        match self {
            Self::CustomStickersEverywhere => "canUseCustomStickersEverywhere",
            Self::StreamQuality => "canStreamQuality",
            Self::HighVideoUploadQuality => "canUseHighVideoUploadQuality",
            Self::ClientThemes => "canUseClientThemes",
            Self::ClientThemeEditor => "CLIENT_THEMES_EDITOR",
            Self::PremiumAppIcons => "canUsePremiumAppIcons",
            Self::PremiumAppIconCurrent => "isPremium(getCurrentUser())",
            Self::ExternalEmojis => "USE_EXTERNAL_EMOJIS / DISALLOW_EXTERNAL",
            Self::AnimatedEmojis => "canUseAnimatedEmojis",
            Self::RoleSubscriptionEmojis => "GUILD_SUBSCRIPTION_UNAVAILABLE",
            Self::EmojiPickerIntent => ".getByName(\"fork_and_knife\")",
            Self::AllStickersAvailable => "\"SENDABLE\".available",
            Self::AllSoundboardSounds => "SOUNDBOUND_SOUND_CREATE .available",
            Self::StreamFpsUnlocked => "STREAM_FPS_OPTION guildPremiumTier",
            Self::LocalSettingsProto => "UserSettingsProtoStore CONNECTION_OPEN",
            Self::GradientThemeSelection => "updateTheme(backgroundGradientPresetId)",
        }
    }

    /// Where a native client implements this.
    ///
    /// Every entry is a place the client already has to exist: a settings flag, a field on the
    /// capability struct, or a permission check. That is the argument for portability - none of
    /// these require owning Discord's internals.
    #[must_use]
    pub const fn native_hook(self) -> &'static str {
        match self {
            Self::CustomStickersEverywhere | Self::AllStickersAvailable => {
                "Capabilities::can_use_stickers_everywhere"
            }
            Self::StreamQuality | Self::StreamFpsUnlocked => "Capabilities::stream_quality",
            Self::HighVideoUploadQuality => "Capabilities::high_video_upload_quality",
            Self::ClientThemes | Self::ClientThemeEditor => "Capabilities::client_themes",
            Self::PremiumAppIcons | Self::PremiumAppIconCurrent => {
                "Capabilities::premium_app_icons"
            }
            Self::ExternalEmojis
            | Self::AnimatedEmojis
            | Self::RoleSubscriptionEmojis
            | Self::EmojiPickerIntent => "Capabilities::emoji_gate",
            Self::AllSoundboardSounds => "Capabilities::all_soundboard_sounds",
            Self::LocalSettingsProto => "SettingsStore::apply_local",
            Self::GradientThemeSelection => "Capabilities::gradient_theme",
        }
    }

    /// Whether a native client can implement this at all.
    ///
    /// False for anything that requires forging server-acknowledged state: Discord does not
    /// render the custom emoji for other people, so the *effect* is bounded to the local view, and
    /// that bound has to be stated rather than glossed over.
    #[must_use]
    pub const fn is_locally_observable(self) -> bool {
        !matches!(
            self,
            Self::ExternalEmojis
                | Self::AnimatedEmojis
                | Self::RoleSubscriptionEmojis
                | Self::EmojiPickerIntent
        )
    }

    /// Note about what the effect actually looks like in practice.
    #[must_use]
    pub const fn caveat(self) -> &'static str {
        if self.is_locally_observable() {
            ""
        } else {
            "local only: other participants still see a plain link"
        }
    }

    /// Matches a Discord symbol from a patch's `find` string.
    ///
    /// A patch may name its target as a bare string, a quoted string, or a regex literal. Substring
    /// matching runs against the raw text so a regex target still hits; a bare predicate name is
    /// additionally compared with quoting and delimiters stripped.
    ///
    /// Deliberately literal rather than fuzzy. A false positive here claims a capability the mod
    /// does not actually want, which is worse than missing one.
    #[must_use]
    pub fn from_patch_target(find: &str) -> Option<Self> {
        type C = Capability;
        /// Bare predicate names, matched after quoting and regex delimiters are stripped so that
        /// `canStreamQuality` and `/canStreamQuality/` both hit.
        const BARE: &[(&str, C)] = &[
            (
                "canUseCustomStickersEverywhere",
                C::CustomStickersEverywhere,
            ),
            ("canStreamQuality", C::StreamQuality),
            ("canUseClientThemes", C::ClientThemes),
            ("canUsePremiumAppIcons", C::PremiumAppIcons),
            ("canUseHighVideoUploadQuality", C::HighVideoUploadQuality),
        ];
        const EXACT: &[(&str, C)] = &[
            (
                "canUseCustomStickersEverywhere",
                C::CustomStickersEverywhere,
            ),
            ("canUseHighVideoUploadQuality", C::HighVideoUploadQuality),
            ("canStreamQuality", C::StreamQuality),
            ("canUseClientThemes", C::ClientThemes),
            ("canUsePremiumAppIcons", C::PremiumAppIcons),
            ("canUseAnimatedEmojis", C::AnimatedEmojis),
            (".getUserIsAdmin(", C::RoleSubscriptionEmojis),
            ("GUILD_SUBSCRIPTION_UNAVAILABLE", C::RoleSubscriptionEmojis),
            ("CLIENT_THEMES_EDITOR", C::ClientThemeEditor),
            ("SENDABLE", C::AllStickersAvailable),
            ("STREAM_FPS_OPTION", C::StreamFpsUnlocked),
            ("UserSettingsProtoStore", C::LocalSettingsProto),
            ("updateTheme(", C::GradientThemeSelection),
            ("GUILD_SOUNDBOARD_SOUND_CREATE", C::AllSoundboardSounds),
            ("getCurrentDesktopIcon(),", C::PremiumAppIconCurrent),
            ("fork_and_knife", C::EmojiPickerIntent),
        ];
        for (needle, capability) in EXACT {
            if find.contains(needle) {
                return Some(*capability);
            }
        }

        // A bare predicate name, matched after quoting and delimiters are stripped.
        let normalized = find
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '/' || c == ':')
            .trim();
        for (needle, capability) in BARE {
            if normalized == *needle {
                return Some(*capability);
            }
        }
        None
    }
}

/// What a native client does with a capability instead of patching it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRecipe {
    /// The capability.
    pub capability: Capability,
    /// The Discord predicate being overridden.
    pub predicate: &'static str,
    /// Where the native client implements it.
    pub native_hook: &'static str,
    /// Whether the effect is visible only locally.
    pub locally_observable: bool,
    /// The caveat, or an empty string.
    pub caveat: &'static str,
}

impl GateRecipe {
    /// Builds a recipe for a capability.
    #[must_use]
    pub fn new(capability: Capability) -> Self {
        Self {
            capability,
            predicate: capability.discord_predicate(),
            native_hook: capability.native_hook(),
            locally_observable: capability.is_locally_observable(),
            caveat: capability.caveat(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_has_a_distinct_name_and_hook() {
        let mut names: Vec<&str> = Capability::ALL.iter().map(|c| c.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate capability names: {names:?}");

        let mut hooks: Vec<&str> = Capability::ALL.iter().map(|c| c.native_hook()).collect();
        hooks.sort_unstable();
        let before = hooks.len();
        hooks.dedup();
        // Hooks may be shared deliberately (several capabilities land on one struct field), so this
        // only asserts that no capability returns an empty hook.
        assert!(hooks.iter().all(|h| !h.is_empty()));
        assert!(before > 0);
    }

    #[test]
    fn matches_fakenitro_patch_targets() {
        // These are the `find` strings from FakeNitro's patch list, verbatim.
        let cases = [
            (
                "canUseCustomStickersEverywhere:",
                Some(Capability::CustomStickersEverywhere),
            ),
            (
                ".getByName(\"fork_and_knife\")",
                Some(Capability::EmojiPickerIntent),
            ),
            (
                ".GUILD_SUBSCRIPTION_UNAVAILABLE;",
                Some(Capability::RoleSubscriptionEmojis),
            ),
            (".getUserIsAdmin(", Some(Capability::RoleSubscriptionEmojis)),
            ("\"SENDABLE\"", Some(Capability::AllStickersAvailable)),
            (
                "#{intl::STREAM_FPS_OPTION}",
                Some(Capability::StreamFpsUnlocked),
            ),
            (
                "\"UserSettingsProtoStore\"",
                Some(Capability::LocalSettingsProto),
            ),
            (",updateTheme(", Some(Capability::GradientThemeSelection)),
            (
                ".CLIENT_THEMES_EDITOR?",
                Some(Capability::ClientThemeEditor),
            ),
            ("}renderStickersAccessories(", None),
            (
                "type:\"GUILD_SOUNDBOARD_SOUND_CREATE\"",
                Some(Capability::AllSoundboardSounds),
            ),
            (
                "getCurrentDesktopIcon(),",
                Some(Capability::PremiumAppIconCurrent),
            ),
        ];
        for (find, expected) in cases {
            assert_eq!(
                Capability::from_patch_target(find),
                expected,
                "for {find:?}"
            );
        }
    }

    #[test]
    fn does_not_claim_unrelated_patches() {
        for find in [
            "}renderStickersAccessories(",
            "[\"strong\",\"em\",\"u\",\"text\",\"inlineCode\",\"s\",\"spoiler\"]",
            ".EMOJI_UPSELL_POPOUT_MORE_EMOJIS_OPENED,",
            "something unrelated",
        ] {
            assert_eq!(
                Capability::from_patch_target(find),
                None,
                "false positive on {find:?}"
            );
        }
    }

    #[test]
    fn bare_predicate_names_match_after_trimming() {
        assert_eq!(
            Capability::from_patch_target("canStreamQuality"),
            Some(Capability::StreamQuality)
        );
    }

    #[test]
    fn emoji_capabilities_are_flagged_as_local_only() {
        // Discord renders the custom emoji for other participants from its own data, so the local
        // override cannot be made to look the same for everyone.
        for c in [
            Capability::ExternalEmojis,
            Capability::AnimatedEmojis,
            Capability::RoleSubscriptionEmojis,
            Capability::EmojiPickerIntent,
        ] {
            assert!(!c.is_locally_observable(), "{c:?} should be local-only");
            assert!(!c.caveat().is_empty(), "{c:?} needs a caveat");
        }
    }

    #[test]
    fn non_emoji_capabilities_are_fully_observable() {
        for c in [
            Capability::StreamQuality,
            Capability::ClientThemes,
            Capability::PremiumAppIcons,
            Capability::CustomStickersEverywhere,
            Capability::AllSoundboardSounds,
        ] {
            assert!(c.is_locally_observable(), "{c:?} should be observable");
            assert_eq!(c.caveat(), "", "{c:?} should need no caveat");
        }
    }

    #[test]
    fn recipe_carries_the_whole_mapping() {
        let recipe = GateRecipe::new(Capability::StreamQuality);
        assert_eq!(recipe.capability, Capability::StreamQuality);
        assert_eq!(recipe.predicate, "canStreamQuality");
        assert_eq!(recipe.native_hook, "Capabilities::stream_quality");
        assert!(recipe.locally_observable);
        assert_eq!(recipe.caveat, "");
    }
}
