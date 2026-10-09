//! Serein's theme schema, as a typed target for converted themes.
//!
//! Serein does not accept CSS. A theme is a `.serein-extension` package holding a small declarative
//! object: 18 colour tokens and 15 control metrics per appearance, plus an optional embedded
//! background image. So converting a BetterDiscord theme is not a translation of selectors — it is
//! a projection of an unbounded CSS variable space onto a fixed, validated token set.
//!
//! That is a much smaller problem than emulating CSS, and it is worth being precise about what it
//! costs: a theme that sets 400 Discord variables will land on at most 36 tokens across both
//! appearances. Fidelity comes from choosing the right token for each variable, not from covering
//! more of them.
//!
//! Source: `docs/theme-api.md` in ViceVerse-cz/Serein. Bounds are enforced at build time, because
//! the host *rejects* a package with an out-of-range metric rather than clamping it.

use dcrs_theme::Rgba;
use serde::{Deserialize, Serialize};

/// The colour tokens Serein's theme API accepts.
///
/// Every field is optional; an omitted token inherits the built-in appearance.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct Colors {
    /// Deepest background: the conversation backdrop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The channel and DM list surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<String>,
    /// The message area.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    /// Cards, popouts and other raised surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raised: Option<String>,
    /// Hovered rows and controls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<String>,
    /// Selected rows and controls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// Dividers and control outlines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    /// High-emphasis text: usernames, headings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_strong: Option<String>,
    /// Body text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// De-emphasised text: timestamps, hints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    /// Hyperlinks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The accent colour: brand buttons, focus rings, selections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Text drawn on `accent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_text: Option<String>,
    /// Positive states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positive: Option<String>,
    /// Warning states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Danger states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger: Option<String>,
    /// Mention pill background.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_bg: Option<String>,
    /// Mention pill text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_text: Option<String>,
}

impl Colors {
    /// Renders a colour for a channel, or `None` when the token is unset.
    #[must_use]
    pub fn get(&self, token: Token) -> Option<&str> {
        let value = match token {
            Token::Base => &self.base,
            Token::Sidebar => &self.sidebar,
            Token::Chat => &self.chat,
            Token::Raised => &self.raised,
            Token::Hover => &self.hover,
            Token::Selected => &self.selected,
            Token::Border => &self.border,
            Token::TextStrong => &self.text_strong,
            Token::Text => &self.text,
            Token::Muted => &self.muted,
            Token::Link => &self.link,
            Token::Accent => &self.accent,
            Token::AccentText => &self.accent_text,
            Token::Positive => &self.positive,
            Token::Warning => &self.warning,
            Token::Danger => &self.danger,
            Token::MentionBg => &self.mention_bg,
            Token::MentionText => &self.mention_text,
        };
        value.as_deref()
    }

    /// Sets a token.
    pub fn set(&mut self, token: Token, hex: String) {
        let slot = match token {
            Token::Base => &mut self.base,
            Token::Sidebar => &mut self.sidebar,
            Token::Chat => &mut self.chat,
            Token::Raised => &mut self.raised,
            Token::Hover => &mut self.hover,
            Token::Selected => &mut self.selected,
            Token::Border => &mut self.border,
            Token::TextStrong => &mut self.text_strong,
            Token::Text => &mut self.text,
            Token::Muted => &mut self.muted,
            Token::Link => &mut self.link,
            Token::Accent => &mut self.accent,
            Token::AccentText => &mut self.accent_text,
            Token::Positive => &mut self.positive,
            Token::Warning => &mut self.warning,
            Token::Danger => &mut self.danger,
            Token::MentionBg => &mut self.mention_bg,
            Token::MentionText => &mut self.mention_text,
        };
        *slot = Some(hex);
    }

    /// How many tokens are set.
    #[must_use]
    pub fn len(&self) -> usize {
        Token::ALL
            .iter()
            .filter(|t| self.get(**t).is_some())
            .count()
    }

    /// Whether no token is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One of the seventeen colour tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Token {
    /// Conversation backdrop.
    Base,
    /// Channel list surface.
    Sidebar,
    /// Message area.
    Chat,
    /// Cards and popouts.
    Raised,
    /// Hovered rows.
    Hover,
    /// Selected rows.
    Selected,
    /// Dividers.
    Border,
    /// High-emphasis text.
    TextStrong,
    /// Body text.
    Text,
    /// De-emphasised text.
    Muted,
    /// Hyperlinks.
    Link,
    /// Brand accent.
    Accent,
    /// Text on the accent.
    AccentText,
    /// Positive state.
    Positive,
    /// Warning state.
    Warning,
    /// Danger state.
    Danger,
    /// Mention pill background.
    MentionBg,
    /// Mention pill text.
    MentionText,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Token {
    /// Every token.
    pub const ALL: [Token; 18] = [
        Token::Base,
        Token::Sidebar,
        Token::Chat,
        Token::Raised,
        Token::Hover,
        Token::Selected,
        Token::Border,
        Token::TextStrong,
        Token::Text,
        Token::Muted,
        Token::Link,
        Token::Accent,
        Token::AccentText,
        Token::Positive,
        Token::Warning,
        Token::Danger,
        Token::MentionBg,
        Token::MentionText,
    ];

    /// The serialized field name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Sidebar => "sidebar",
            Self::Chat => "chat",
            Self::Raised => "raised",
            Self::Hover => "hover",
            Self::Selected => "selected",
            Self::Border => "border",
            Self::TextStrong => "text_strong",
            Self::Text => "text",
            Self::Muted => "muted",
            Self::Link => "link",
            Self::Accent => "accent",
            Self::AccentText => "accent_text",
            Self::Positive => "positive",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::MentionBg => "mention_bg",
            Self::MentionText => "mention_text",
        }
    }

    /// The Serein area this token belongs to.
    #[must_use]
    pub const fn area(self) -> &'static str {
        match self {
            Self::Base
            | Self::Sidebar
            | Self::Chat
            | Self::Raised
            | Self::Hover
            | Self::Selected
            | Self::Border => "surfaces",
            Self::TextStrong | Self::Text | Self::Muted | Self::Link => "text",
            Self::Accent | Self::AccentText | Self::Positive | Self::Warning | Self::Danger => {
                "actions and states"
            }
            Self::MentionBg | Self::MentionText => "mentions",
        }
    }
}

/// A control metric with its allowed inclusive range.
///
/// The host rejects an out-of-range value rather than clamping, so the range is enforced here and
/// the offending declaration is reported instead of silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricRange {
    /// Metric name.
    pub name: &'static str,
    /// Lowest accepted value.
    pub min: i32,
    /// Highest accepted value.
    pub max: i32,
    /// Serein's default, used when a theme does not set it.
    pub default: i32,
}

/// Every metric Serein's theme API accepts, with its bounds.
///
/// Source: the table in `docs/theme-api.md`. The `[a, b]` entries there are per-axis pairs and are
/// represented as two metrics here.
pub const METRICS: &[MetricRange] = &[
    MetricRange {
        name: "body_size",
        min: 10,
        max: 28,
        default: 15,
    },
    MetricRange {
        name: "heading_size",
        min: 12,
        max: 40,
        default: 20,
    },
    MetricRange {
        name: "button_size",
        min: 10,
        max: 28,
        default: 14,
    },
    MetricRange {
        name: "small_size",
        min: 10,
        max: 28,
        default: 12,
    },
    MetricRange {
        name: "monospace_size",
        min: 10,
        max: 28,
        default: 14,
    },
    MetricRange {
        name: "item_spacing_x",
        min: 0,
        max: 24,
        default: 8,
    },
    MetricRange {
        name: "item_spacing_y",
        min: 0,
        max: 24,
        default: 8,
    },
    MetricRange {
        name: "button_padding_x",
        min: 0,
        max: 24,
        default: 12,
    },
    MetricRange {
        name: "button_padding_y",
        min: 0,
        max: 24,
        default: 6,
    },
    MetricRange {
        name: "control_height",
        min: 24,
        max: 56,
        default: 32,
    },
    MetricRange {
        name: "widget_radius",
        min: 0,
        max: 24,
        default: 8,
    },
    MetricRange {
        name: "window_radius",
        min: 0,
        max: 24,
        default: 12,
    },
    MetricRange {
        name: "menu_radius",
        min: 0,
        max: 24,
        default: 12,
    },
    MetricRange {
        name: "transparency",
        min: 0,
        max: 100,
        default: 0,
    },
    MetricRange {
        name: "blur",
        min: 0,
        max: 100,
        default: 0,
    },
];

/// The bounds for a metric.
#[must_use]
pub fn metric(name: &str) -> Option<&'static MetricRange> {
    METRICS.iter().find(|m| m.name == name)
}

/// The control-metric half of a theme.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    /// Whether a theme may request compositor blur.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency_blur: Option<bool>,
    /// Desktop visibility, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency: Option<u8>,
    /// Native compositor blur, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<u8>,
    /// Body font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_size: Option<u8>,
    /// Heading font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_size: Option<u8>,
    /// Button font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_size: Option<u8>,
    /// Small label font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_size: Option<u8>,
    /// Monospace font size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monospace_size: Option<u8>,
    /// Vertical item spacing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_spacing: Option<[u8; 2]>,
    /// Horizontal item spacing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_padding: Option<[u8; 2]>,
    /// Shared control height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_height: Option<u8>,
    /// Widget corner radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget_radius: Option<u8>,
    /// Window corner radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_radius: Option<u8>,
    /// Menu corner radius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_radius: Option<u8>,
}

impl Style {
    /// How many metrics are set.
    #[must_use]
    pub fn len(&self) -> usize {
        let mut n = 0;
        if self.transparency_blur.is_some() {
            n += 1;
        }
        for v in [
            self.transparency,
            self.blur,
            self.body_size,
            self.heading_size,
            self.button_size,
            self.small_size,
            self.monospace_size,
            self.item_spacing.map(|v| v[0]),
            self.item_spacing.map(|v| v[1]),
            self.button_padding.map(|v| v[0]),
            self.button_padding.map(|v| v[1]),
            self.control_height,
            self.widget_radius,
            self.window_radius,
            self.menu_radius,
        ] {
            n += usize::from(v.is_some());
        }
        n
    }

    /// Whether no metric is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One appearance (light or dark).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    /// Colour tokens.
    #[serde(skip_serializing_if = "Colors::is_empty")]
    pub colors: Colors,
    /// Two-colour top-to-bottom backdrop gradient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop: Option<[String; 2]>,
    /// Embedded background image settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
}

/// How the image is scaled into its target area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Centre crop to fill the area.
    #[default]
    Cover,
    /// Whole image, centred.
    Contain,
}

/// Where the image paints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// The whole-window backdrop.
    #[default]
    Window,
    /// Only the message area.
    Chat,
}

/// Per-surface coverage over one continuous image.
///
/// Text and controls stay opaque; these percentages only affect surface fills, so changing one
/// section does not disturb its neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SectionOpacity {
    /// Title and channel bar.
    pub top_bar: u8,
    /// Server rail.
    pub server_list: u8,
    /// DM and channel list.
    pub channel_list: u8,
    /// Message list.
    pub message_list: u8,
    /// Member and search list.
    pub member_list: u8,
    /// Message input area.
    pub composer: u8,
}

impl Default for SectionOpacity {
    fn default() -> Self {
        Self {
            top_bar: 85,
            server_list: 85,
            channel_list: 85,
            message_list: 75,
            member_list: 85,
            composer: 90,
        }
    }
}

impl SectionOpacity {
    /// Every section value, for range checking.
    #[must_use]
    pub fn values(self) -> [u8; 6] {
        [
            self.top_bar,
            self.server_list,
            self.channel_list,
            self.message_list,
            self.member_list,
            self.composer,
        ]
    }
}

/// Background image settings.
///
/// The bytes themselves are not here: a package carries exactly one `background_image` array at
/// the top level, and both palettes' settings apply to that single image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Background {
    /// Image opacity, 0–100. Defaults to 25, as the host's editor does.
    pub opacity: u8,
    /// How the image is scaled.
    pub fit: Fit,
    /// Where it paints.
    pub target: Target,
    /// Independent per-surface coverage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<SectionOpacity>,
}

impl Default for Background {
    fn default() -> Self {
        Self {
            opacity: DEFAULT_OPACITY,
            fit: Fit::Cover,
            target: Target::Window,
            sections: None,
        }
    }
}

/// The opacity Serein's editor uses when an image is first chosen.
///
/// Not zero: a theme background that starts fully opaque hides the surfaces it was meant to sit
/// behind, which reads as a conversion bug rather than a setting.
pub const DEFAULT_OPACITY: u8 = 25;

/// A theme: two appearances plus shared control metrics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    /// Light appearance.
    #[serde(skip_serializing_if = "Appearance::is_empty")]
    pub light: Appearance,
    /// Dark appearance.
    #[serde(skip_serializing_if = "Appearance::is_empty")]
    pub dark: Appearance,
    /// Shared control metrics.
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

impl Appearance {
    /// Whether the appearance would paint nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty() && self.backdrop.is_none() && self.background.is_none()
    }
}

impl Theme {
    /// How many colour tokens are set across both appearances.
    #[must_use]
    pub fn token_count(&self) -> usize {
        self.light.colors.len() + self.dark.colors.len()
    }

    /// Whether the theme would change anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.token_count() == 0 && self.style.is_empty()
    }

    /// Picks black or white for text on `background`, whichever is more legible.
    ///
    /// Needed because themes routinely set `accent` without setting `accent_text`, and Serein
    /// requires that pair to be legible. Deriving it beats leaving the host's default, which may not
    /// match the theme's accent at all.
    pub fn derive_accent_text(&mut self) {
        for appearance in [&mut self.light, &mut self.dark] {
            let Some(accent) = appearance.colors.get(Token::Accent) else {
                continue;
            };
            if appearance.colors.get(Token::AccentText).is_some() {
                continue;
            }
            let Ok(rgba) = dcrs_theme::color::evaluate(accent) else {
                continue;
            };
            let text = if rgba.luminance() > 0.5 {
                "#000000"
            } else {
                "#ffffff"
            };
            appearance.colors.set(Token::AccentText, text.to_owned());
        }
    }

    /// Validates every metric against Serein's ranges.
    ///
    /// Returns one error per offending metric. Serein rejects such a package outright, so this is
    /// a hard error rather than a clamp.
    ///
    /// # Errors
    /// Returns [`ThemeError::MetricOutOfRange`] for each metric outside its documented bounds.
    pub fn validate(&self) -> Result<(), Vec<ThemeError>> {
        let mut errors: Vec<ThemeError> = Vec::new();
        let mut check = |name: &str, value: Option<i32>| {
            let (Some(value), Some(range)) = (value, metric(name)) else {
                return;
            };
            if value < range.min || value > range.max {
                errors.push(ThemeError::MetricOutOfRange {
                    name: name.to_owned(),
                    value,
                    min: range.min,
                    max: range.max,
                });
            }
        };
        let s = &self.style;
        check("body_size", s.body_size.map(i32::from));
        check("heading_size", s.heading_size.map(i32::from));
        check("button_size", s.button_size.map(i32::from));
        check("small_size", s.small_size.map(i32::from));
        check("monospace_size", s.monospace_size.map(i32::from));
        check("control_height", s.control_height.map(i32::from));
        check("widget_radius", s.widget_radius.map(i32::from));
        check("window_radius", s.window_radius.map(i32::from));
        check("transparency", s.transparency.map(i32::from));
        check("blur", s.blur.map(i32::from));
        if let Some([x, _]) = s.item_spacing {
            check("item_spacing_x", Some(i32::from(x)));
        }
        if let Some([_, y]) = s.item_spacing {
            check("item_spacing_y", Some(i32::from(y)));
        }
        if let Some([x, _]) = s.button_padding {
            check("button_padding_x", Some(i32::from(x)));
        }
        if let Some([_, y]) = s.button_padding {
            check("button_padding_y", Some(i32::from(y)));
        }

        for (label, colors) in [("light", &self.light.colors), ("dark", &self.dark.colors)] {
            for token in Token::ALL {
                let Some(value) = colors.get(token) else {
                    continue;
                };
                if dcrs_theme::color::evaluate(value).is_err() {
                    errors.push(ThemeError::InvalidColor {
                        appearance: label.to_owned(),
                        token: token.as_str().to_owned(),
                        value: value.to_owned(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Problems that make a theme unacceptable to Serein.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ThemeError {
    /// A metric is outside the range the host accepts.
    #[error("metric {name} = {value} is outside {min}..={max}")]
    MetricOutOfRange {
        /// Metric name.
        name: String,
        /// Offending value.
        value: i32,
        /// Lowest accepted.
        min: i32,
        /// Highest accepted.
        max: i32,
    },
    /// A colour token is not a colour Serein can parse.
    #[error("{appearance}.colors.{token} is not a valid colour: {value:?}")]
    InvalidColor {
        /// `light` or `dark`.
        appearance: String,
        /// Token name.
        token: String,
        /// Offending value.
        value: String,
    },
}

/// A rendered colour, ready to be written into the token set.
#[must_use]
pub fn hex(color: Rgba) -> String {
    color.to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique() {
        let mut names: Vec<&str> = Token::ALL.iter().map(|t| t.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate token names: {names:?}");
    }

    #[test]
    fn every_token_round_trips_through_get_and_set() {
        let mut colors = Colors::default();
        for token in Token::ALL {
            colors.set(token, "#123456".to_owned());
        }
        for token in Token::ALL {
            assert_eq!(
                colors.get(token),
                Some("#123456"),
                "round-trip failed for {token:?}"
            );
        }
        assert_eq!(colors.len(), Token::ALL.len());
    }

    #[test]
    fn unset_tokens_are_absent_not_empty() {
        let colors = Colors::default();
        assert!(colors.is_empty());
        assert_eq!(colors.get(Token::Accent), None);
        // An unset token must be omitted from JSON so the host inherits its default.
        assert!(!serde_json::to_string(&colors).unwrap().contains("accent"));
    }

    #[test]
    fn accent_text_is_derived_when_absent() {
        let mut theme = Theme {
            dark: Appearance {
                colors: Colors {
                    accent: Some("#5865f2".to_owned()),
                    ..Colors::default()
                },
                ..Appearance::default()
            },
            ..Theme::default()
        };
        theme.derive_accent_text();
        // Discord's blurple is dark, so white text is the legible choice.
        assert_eq!(theme.dark.colors.get(Token::AccentText), Some("#ffffff"));
    }

    #[test]
    fn derived_accent_text_does_not_override_an_explicit_choice() {
        let mut theme = Theme {
            dark: Appearance {
                colors: Colors {
                    accent: Some("#5865f2".to_owned()),
                    accent_text: Some("#ff00ff".to_owned()),
                    ..Colors::default()
                },
                ..Appearance::default()
            },
            ..Theme::default()
        };
        theme.derive_accent_text();
        assert_eq!(theme.dark.colors.get(Token::AccentText), Some("#ff00ff"));
    }

    #[test]
    fn metric_bounds_match_the_documented_table() {
        assert_eq!(metric("body_size").unwrap().default, 15);
        assert_eq!(metric("body_size").unwrap().min, 10);
        assert_eq!(metric("control_height").unwrap().max, 56);
        assert_eq!(metric("widget_radius").unwrap().min, 0);
        assert_eq!(metric("transparency").unwrap().max, 100);
        assert_eq!(metric("nonexistent"), None);
    }

    #[test]
    fn a_default_theme_validates() {
        assert!(Theme::default().validate().is_ok());
    }

    #[test]
    fn out_of_range_metrics_are_rejected_not_clamped() {
        // Serein refuses such a package outright, so clamping would hide a real rejection.
        let theme = Theme {
            style: Style {
                body_size: Some(200),
                ..Style::default()
            },
            ..Theme::default()
        };
        let errors = theme.validate().unwrap_err();
        assert!(
            errors.iter().any(|e| matches!(
                e,
                ThemeError::MetricOutOfRange { name, min: 10, max: 28, .. } if name == "body_size"
            )),
            "got {errors:?}"
        );
    }

    #[test]
    fn pair_metrics_are_validated_per_axis() {
        let ok = Theme {
            style: Style {
                item_spacing: Some([8, 24]),
                ..Style::default()
            },
            ..Theme::default()
        };
        assert!(ok.validate().is_ok(), "24 is the documented maximum");

        let bad = Theme {
            style: Style {
                item_spacing: Some([8, 40]),
                ..Style::default()
            },
            ..Theme::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn unparseable_colours_are_rejected() {
        let theme = Theme {
            dark: Appearance {
                colors: Colors {
                    accent: Some("oklch(0.7 0.1 200)".to_owned()),
                    ..Colors::default()
                },
                ..Appearance::default()
            },
            ..Theme::default()
        };
        let errors = theme.validate().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ThemeError::InvalidColor { .. }))
        );
    }

    #[test]
    fn serialization_omits_empty_structures() {
        let theme = Theme {
            dark: Appearance {
                colors: Colors {
                    chat: Some("#1e1f22".to_owned()),
                    ..Colors::default()
                },
                ..Appearance::default()
            },
            ..Theme::default()
        };
        let json = serde_json::to_string(&theme).unwrap();
        assert!(json.contains("\"chat\":\"#1e1f22\""), "{json}");
        // Light was left empty; an all-empty appearance object should not dominate the package.
        assert!(!json.contains("\"backdrop\""), "{json}");
    }
}
