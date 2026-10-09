//! Projection from Discord CSS variables onto Serein's native theme tokens.

use std::collections::BTreeMap;

use dcrs_theme::{ColorError, Rgba, color};

use crate::theme::{Colors, MetricRange, Style, Theme, Token};

/// How a token is derived from the theme's palette when no variable resolves to it.
#[derive(Debug, Clone, Copy)]
struct SurfaceFallback {
    /// The variable to derive from.
    base: &'static str,
    /// The variable the theme would have set directly, if it had.
    ///
    /// Only used to avoid overriding a value that is already readable and distinct.
    target: &'static str,
    /// How far to step from `base`, as a multiplier.
    ///
    /// Distinct per surface on purpose. Deriving every one of them by the same step produces the
    /// same colour for all of them, which is the flat result this exists to prevent.
    step: f32,
}

/// Discord custom property candidates for one Serein colour token.
struct TokenSources {
    /// The token being filled.
    token: Token,
    /// `--variable` names, tried in order.
    variables: &'static [&'static str],
    /// Whether to synthesize the token from the palette when no variable resolves.
    ///
    /// `base`, `sidebar` and `chat` are the three surfaces a theme always paints, and Discord
    /// distinguishes four or five near-identical variables for each. Without this a monochrome theme
    /// would land with only half its surfaces set.
    fallback: Option<SurfaceFallback>,
}

/// The colour mapping table.
const COLOR_SOURCES: &[TokenSources] = &[
    TokenSources {
        token: Token::Base,
        variables: &[
            "--background-deep",
            "--bg-base-primary",
            "--chat-background",
            "--background-primary",
            "--base",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::Sidebar,
        variables: &[
            "--bg-base-tertiary",
            "--background-secondary",
            "--background-secondary-alt",
            "--sidebar",
        ],
        fallback: Some(SurfaceFallback {
            base: "--background-primary",
            target: "--background-secondary",
            step: 0.06,
        }),
    },
    TokenSources {
        token: Token::Chat,
        // `--background-primary` is deliberately absent. Discord paints it behind the whole window,
        // so a theme that sets it and nothing else would land chat and base on the identical colour —
        // the flat, unreadable result the fallback exists to prevent. Deriving chat from base keeps
        // them distinguishable.
        variables: &["--chat-background", "--bg-base-secondary"],
        fallback: Some(SurfaceFallback {
            base: "--background-primary",
            target: "--chat-background",
            step: 0.12,
        }),
    },
    TokenSources {
        token: Token::Raised,
        variables: &[
            "--bg-surface-overlay",
            "--background-floating",
            "--background-elevated",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::Hover,
        variables: &[
            "--background-modifier-hover",
            "--bg-mod-subtle",
            "--background-message-hover",
        ],
        // A theme that never mentions hover still has one: Discord's default is a translucent
        // white, and leaving the token unset means the host's built-in shows instead.
        fallback: Some(SurfaceFallback {
            base: "--background-primary",
            target: "--background-modifier-hover",
            step: 0.18,
        }),
    },
    TokenSources {
        token: Token::Selected,
        variables: &[
            "--background-modifier-selected",
            "--bg-mod-strong",
            "--background-modifier-accent",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::Border,
        variables: &[
            "--border-strong",
            "--divider-strong",
            "--bg-base-tertiary",
            "--border-subtle",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::TextStrong,
        variables: &["--header-primary", "--text-primary", "--text-normal"],
        fallback: None,
    },
    TokenSources {
        token: Token::Text,
        variables: &["--text-normal", "--channels-default", "--message-text"],
        fallback: None,
    },
    TokenSources {
        token: Token::Muted,
        variables: &["--text-muted", "--channels-default", "--header-secondary"],
        fallback: None,
    },
    TokenSources {
        token: Token::Link,
        variables: &["--text-link", "--text-link-low-saturation", "--link-color"],
        fallback: None,
    },
    TokenSources {
        token: Token::Accent,
        variables: &[
            "--brand-experiment",
            "--brand-experiment-567",
            "--brand",
            "--accent",
            "--interactive-accent",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::Positive,
        variables: &[
            "--status-positive",
            "--positive",
            "--success",
            "--status-online",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::Warning,
        variables: &["--status-warning", "--warning", "--status-idle"],
        fallback: None,
    },
    TokenSources {
        token: Token::Danger,
        variables: &[
            "--status-danger",
            "--danger",
            "--destructive",
            "--status-dnd",
            "--text-danger",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::MentionBg,
        variables: &[
            "--mention-background",
            "--mention-bg",
            "--background-mention",
        ],
        fallback: None,
    },
    TokenSources {
        token: Token::MentionText,
        variables: &["--mention-foreground", "--mention-text", "--mention-color"],
        fallback: None,
    },
];

/// Discord custom property candidates for one Serein metric.
struct MetricSources {
    /// Serein metric name.
    metric: &'static str,
    /// `--variable` names, tried in order.
    variables: &'static [&'static str],
    /// When true the value is a length in `px`; when false it is a unitless number or percentage.
    length: bool,
}

/// The metric mapping table.
const METRIC_SOURCES: &[MetricSources] = &[
    MetricSources {
        metric: "body_size",
        variables: &["--font-size-md", "--font-size-normal", "--font-size-16"],
        length: true,
    },
    MetricSources {
        metric: "heading_size",
        variables: &["--font-size-2xl", "--font-size-xl", "--font-size-20"],
        length: true,
    },
    MetricSources {
        metric: "button_size",
        variables: &["--font-size-sm", "--font-size-14"],
        length: true,
    },
    MetricSources {
        metric: "small_size",
        variables: &["--font-size-xs", "--font-size-12", "--font-size-10"],
        length: true,
    },
    MetricSources {
        metric: "monospace_size",
        variables: &["--font-size-mono", "--font-size-code"],
        length: true,
    },
    MetricSources {
        metric: "item_spacing_x",
        variables: &["--spacing-8", "--spacing-12"],
        length: true,
    },
    MetricSources {
        metric: "item_spacing_y",
        variables: &["--spacing-4", "--spacing-8"],
        length: true,
    },
    MetricSources {
        metric: "button_padding_x",
        variables: &["--spacing-12", "--spacing-8"],
        length: true,
    },
    MetricSources {
        metric: "button_padding_y",
        variables: &["--spacing-8", "--spacing-4"],
        length: true,
    },
    MetricSources {
        metric: "control_height",
        variables: &["--control-height", "--spacing-32", "--spacing-40"],
        length: true,
    },
    MetricSources {
        metric: "widget_radius",
        variables: &["--radius-sm", "--radius-md"],
        length: true,
    },
    MetricSources {
        metric: "window_radius",
        variables: &["--radius-lg", "--radius-xl", "--radius-2xl"],
        length: true,
    },
    MetricSources {
        metric: "menu_radius",
        variables: &["--radius-md", "--radius-lg"],
        length: true,
    },
    MetricSources {
        metric: "transparency",
        variables: &["--transparency", "--bg-opacity", "--background-opacity"],
        length: false,
    },
    MetricSources {
        metric: "blur",
        variables: &["--blur", "--background-blur"],
        length: false,
    },
];

/// A declaration that could not be projected onto a token.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    /// The Serein token or metric that was wanted.
    pub target: String,
    /// The Discord variable that was tried.
    pub variable: String,
    /// Why it did not land.
    pub reason: String,
}

/// The outcome of a conversion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conversion {
    /// The theme to emit.
    pub theme: Theme,
    /// How many Serein tokens are set across both appearances.
    pub tokens_mapped: usize,
    /// How many Discord variables were consulted.
    pub variables_seen: usize,
    /// Declarations that did not land, for the report.
    pub dropped: Vec<Dropped>,
}

impl Conversion {
    /// Fraction of Serein's colour tokens that were filled.
    ///
    /// Denominator is `18 * 2` — every token in both appearances — because a theme that sets only
    /// dark-mode variables genuinely does not light-mode them.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        let max = Token::ALL.len() * 2;
        #[allow(clippy::cast_precision_loss)]
        {
            self.tokens_mapped as f64 / max as f64
        }
    }
}

/// Converts a resolved variable map into a Serein theme.
///
/// `light` and `dark` are the variables declared under `.theme-light` and `.theme-dark`, with
/// `root` as the base that both inherit. Values must already be `var()`-resolved.
#[must_use]
pub fn convert(root: &BTreeMap<String, String>, dark: &BTreeMap<String, String>) -> Conversion {
    let mut conversion = Conversion::default();
    conversion.theme.light.colors = project_colors(root, "light", &mut conversion);
    conversion.theme.dark.colors = project_colors(dark, "dark", &mut conversion);
    conversion.theme.style = project_style(root, dark, &mut conversion);
    // The host needs a legible text colour on the accent, and a theme rarely sets the pair.
    conversion.theme.derive_accent_text();
    conversion.tokens_mapped = conversion.theme.token_count();
    conversion
}

/// Builds the colour tokens for one appearance.
fn project_colors(
    vars: &BTreeMap<String, String>,
    label: &str,
    conversion: &mut Conversion,
) -> Colors {
    let mut colors = Colors::default();

    for source in COLOR_SOURCES {
        let resolved = resolve_color(vars, source.variables);
        if let Resolved::Found(hex) = resolved {
            colors.set(source.token, hex);
            continue;
        }

        // Nothing readable directly. A surface can still be derived from the theme's own palette,
        // which is what keeps a monochrome theme from producing one flat colour where the host expects
        // several distinct ones. This applies whether the candidate was absent or unreadable: a theme
        // that never mentions a sidebar still has one.
        if let Some(hex) = source
            .fallback
            .and_then(|fallback| derive_surface(vars, fallback))
        {
            colors.set(source.token, hex);
            continue;
        }

        // Only a variable the theme actually declared is worth reporting. A token with no candidate
        // present is not a loss; it means the host's own default applies.
        if let Resolved::Unresolved { name, error } = resolved {
            conversion.dropped.push(Dropped {
                target: format!("{label}.colors.{}", source.token.as_str()),
                variable: name.to_owned(),
                reason: describe_color_error(&error),
            });
        }
    }

    colors
}

/// Outcome of looking for a colour.
enum Resolved {
    /// A candidate resolved and evaluated.
    Found(String),
    /// Candidates were declared but none evaluated.
    ///
    /// Carries the first name seen, because that is the variable the author actually wrote, and the
    /// error from the last one tried, because that is what the report needs to explain the failure.
    Unresolved {
        /// The first declared candidate name.
        name: &'static str,
        /// Why the last attempt failed.
        error: ColorError,
    },
    /// No candidate was declared at all, so nothing is dropped.
    Absent,
}

fn resolve_color(vars: &BTreeMap<String, String>, candidates: &'static [&'static str]) -> Resolved {
    let mut declared = None;
    let mut last_error = None;
    for name in candidates {
        let Some(value) = vars.get(*name) else {
            continue;
        };
        if declared.is_none() {
            declared = Some(*name);
        }
        match color::evaluate(value) {
            Ok(c) => return Resolved::Found(c.to_hex()),
            Err(e) => last_error = Some(e),
        }
    }
    match (declared, last_error) {
        (Some(name), Some(error)) => Resolved::Unresolved { name, error },
        (Some(name), None) => Resolved::Unresolved {
            name,
            error: ColorError::Unsupported("colour".to_owned()),
        },
        (None, _) => Resolved::Absent,
    }
}

/// Derives a surface from the theme's own palette, stepping away from `base`.
///
/// Discord's own light and dark defaults differ from the base by a small step, so a surface a step off
/// the base reads correctly against it. That is what a theme author does by hand, and it is what keeps
/// a theme from producing one flat surface where Serein expects several distinct ones.
///
/// Scaling rather than adding an offset preserves hue, which matters more than the exact step: a
/// themed sidebar that shifts hue stops reading as part of the same palette. The step is per-surface so
/// that deriving several of them yields several distinct colours rather than the same one twice.
///
/// `target` is consulted only to confirm the theme intended a distinct surface: when it declared the
/// target itself and that value resolved, there is nothing to derive.
fn derive_surface(vars: &BTreeMap<String, String>, fallback: SurfaceFallback) -> Option<String> {
    let base = color::evaluate(vars.get(fallback.base)?).ok()?;
    // Already readable and distinct: use what the author wrote rather than a guess. Compared as colours
    // rather than as hex strings, so this costs no allocation.
    if let Some(target) = vars
        .get(fallback.target)
        .and_then(|v| color::evaluate(v).ok())
    {
        if target != base {
            return Some(target.to_hex());
        }
    }

    let up = if base.luminance() < 0.5 {
        1.0 + fallback.step
    } else {
        1.0 - fallback.step
    };
    let scale = |c: u8| (f32::from(c) * up).clamp(0.0, 255.0).round() as u8;
    Some(Rgba::rgb(scale(base.r), scale(base.g), scale(base.b)).to_hex())
}

/// Builds the shared control metrics, preferring dark variables but falling back to root.
fn project_style(
    root: &BTreeMap<String, String>,
    dark: &BTreeMap<String, String>,
    conversion: &mut Conversion,
) -> Style {
    let mut style = Style::default();

    for source in METRIC_SOURCES {
        // Borrowed rather than cloned: each metric probes two or three candidate variables, and the
        // value was being copied out of the map for every probe including the ones that missed.
        let merged = |name: &str| {
            dark.get(name)
                .or_else(|| root.get(name))
                .map(String::as_str)
        };
        let Some((name, raw)) = source
            .variables
            .iter()
            .find_map(|v| merged(v).map(|value| (*v, value)))
        else {
            continue;
        };

        let Some(range) = crate::theme::metric(source.metric) else {
            continue;
        };

        match numeric_value(raw, source.length) {
            Some(value) if in_range(value, range) => apply_metric(&mut style, source.metric, value),
            Some(value) => conversion.dropped.push(Dropped {
                target: format!("style.{}", source.metric),
                // The variable name, not the value: a report that says `font-size-md` is 999px is
                // actionable, and one that says `999px` is not.
                variable: name.to_owned(),
                reason: format!("{value} is outside {}..={}", range.min, range.max),
            }),
            None => conversion.dropped.push(Dropped {
                target: format!("style.{}", source.metric),
                variable: name.to_owned(),
                reason: format!("{raw:?} is not a number"),
            }),
        }
    }

    style
}

/// Extracts a number from a CSS value, stripping units when `length`.
fn numeric_value(raw: &str, length: bool) -> Option<i32> {
    let t = raw.trim();
    if let Some(v) = t.strip_suffix('%') {
        return v.trim().parse::<f32>().ok().map(|n| n.round() as i32);
    }
    if length {
        if let Some(v) = t.strip_suffix("px") {
            return v.trim().parse::<f32>().ok().map(|n| n.round() as i32);
        }
        if let Some(v) = t.strip_suffix("rem") {
            return v
                .trim()
                .parse::<f32>()
                .ok()
                .map(|n| (n * 16.0).round() as i32);
        }
        if let Some(v) = t.strip_suffix("em") {
            return v
                .trim()
                .parse::<f32>()
                .ok()
                .map(|n| (n * 16.0).round() as i32);
        }
    }
    t.parse::<f32>().ok().map(|n| n.round() as i32)
}

fn in_range(value: i32, range: &MetricRange) -> bool {
    value >= range.min && value <= range.max
}

fn apply_metric(style: &mut Style, name: &str, value: i32) {
    let v = u8::try_from(value).unwrap_or(0);
    match name {
        "body_size" => style.body_size = Some(v),
        "heading_size" => style.heading_size = Some(v),
        "button_size" => style.button_size = Some(v),
        "small_size" => style.small_size = Some(v),
        "monospace_size" => style.monospace_size = Some(v),
        "control_height" => style.control_height = Some(v),
        "widget_radius" => style.widget_radius = Some(v),
        "window_radius" => style.window_radius = Some(v),
        "menu_radius" => style.menu_radius = Some(v),
        "transparency" => style.transparency = Some(v),
        "blur" => style.blur = Some(v),
        // Per-axis metrics accumulate into the pair, taking whichever axis is set first.
        "item_spacing_x" => {
            let entry = style.item_spacing.get_or_insert([0, 0]);
            entry[0] = v;
        }
        "item_spacing_y" => {
            let entry = style.item_spacing.get_or_insert([0, 0]);
            entry[1] = v;
        }
        "button_padding_x" => {
            let entry = style.button_padding.get_or_insert([0, 0]);
            entry[0] = v;
        }
        "button_padding_y" => {
            let entry = style.button_padding.get_or_insert([0, 0]);
            entry[1] = v;
        }
        _ => {}
    }
}

/// A convenience for callers that want a legible text colour on a given background.
#[must_use]
pub fn text_on(background: Rgba) -> String {
    if background.luminance() > 0.5 {
        "#000000".to_owned()
    } else {
        "#ffffff".to_owned()
    }
}

/// Colour evaluation errors that the conversion should surface in its report.
#[must_use]
pub fn describe_color_error(error: &ColorError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn maps_discord_names_onto_serein_tokens() {
        let dark = vars(&[
            ("--background-primary", "#1e1f22"),
            ("--background-secondary", "#2b2d31"),
            ("--chat-background", "#111214"),
            ("--background-floating", "#313338"),
            ("--text-normal", "#dbdee1"),
            ("--text-muted", "#949ba4"),
            ("--text-link", "#00a8fc"),
            ("--brand-experiment", "#5865f2"),
            ("--status-positive", "#3ba55d"),
            ("--status-danger", "#ed4245"),
            ("--mention-background", "#facc15"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        let c = &out.theme.dark.colors;

        assert_eq!(c.get(Token::Text), Some("#dbdee1"));
        assert_eq!(c.get(Token::Muted), Some("#949ba4"));
        assert_eq!(c.get(Token::Link), Some("#00a8fc"));
        assert_eq!(c.get(Token::Accent), Some("#5865f2"));
        assert_eq!(c.get(Token::Positive), Some("#3ba55d"));
        assert_eq!(c.get(Token::Danger), Some("#ed4245"));
        assert_eq!(c.get(Token::MentionBg), Some("#facc15"));
        assert_eq!(c.get(Token::Raised), Some("#313338"));
    }

    #[test]
    fn evaluates_hsl_and_alpha_expressions() {
        // The realistic case: a theme using hsl() and a translucent accent.
        let dark = vars(&[
            ("--brand-experiment-hsl", "235 86% 66%"),
            ("--background-modifier-hover", "hsl(220 6% 20% / 0.6)"),
            ("--text-link", "rgb(0 168 252)"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        // The `-hsl` form is not a colour on its own, so the base accent must come from elsewhere.
        // 0.6 alpha is 153/255 = 0x99.
        assert_eq!(out.theme.dark.colors.get(Token::Hover), Some("#30323699"));
        assert_eq!(out.theme.dark.colors.get(Token::Link), Some("#00a8fc"));
    }

    #[test]
    fn accent_text_is_derived_for_legibility() {
        let dark = vars(&[("--brand-experiment", "#5865f2")]);
        let out = convert(&BTreeMap::new(), &dark);
        assert_eq!(out.theme.dark.colors.get(Token::Accent), Some("#5865f2"));
        assert_eq!(
            out.theme.dark.colors.get(Token::AccentText),
            Some("#ffffff")
        );
    }

    #[test]
    fn light_accent_gets_dark_text() {
        let light = vars(&[("--brand-experiment", "#ffe066")]);
        let out = convert(&light, &BTreeMap::new());
        assert_eq!(
            out.theme.light.colors.get(Token::AccentText),
            Some("#000000")
        );
    }

    #[test]
    fn first_matching_candidate_wins() {
        // `--background-secondary` is more specific than `--background-secondary-alt`.
        let dark = vars(&[
            ("--background-secondary", "#111111"),
            ("--background-secondary-alt", "#222222"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        assert_eq!(out.theme.dark.colors.get(Token::Sidebar), Some("#111111"));
    }

    #[test]
    fn unresolvable_colours_are_reported_not_guessed() {
        let dark = vars(&[("--text-normal", "oklch(0.7 0.1 200)")]);
        let out = convert(&BTreeMap::new(), &dark);
        assert!(out.theme.dark.colors.get(Token::Text).is_none());
        assert!(
            out.dropped.iter().any(|d| d.target.contains("colors.text")),
            "{:?}",
            out.dropped
        );
    }

    #[test]
    fn maps_typography_metrics() {
        let dark = vars(&[
            ("--font-size-md", "16px"),
            ("--font-size-2xl", "24px"),
            ("--font-size-sm", "13px"),
            ("--font-size-xs", "11px"),
            ("--radius-sm", "4px"),
            ("--control-height", "40px"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        let s = &out.theme.style;
        assert_eq!(s.body_size, Some(16));
        assert_eq!(s.heading_size, Some(24));
        assert_eq!(s.button_size, Some(13));
        assert_eq!(s.small_size, Some(11));
        assert_eq!(s.widget_radius, Some(4));
        assert_eq!(s.control_height, Some(40));
        assert!(out.theme.validate().is_ok());
    }

    #[test]
    fn rem_is_converted_to_logical_pixels() {
        let dark = vars(&[("--font-size-md", "1.125rem")]);
        let out = convert(&BTreeMap::new(), &dark);
        assert_eq!(out.theme.style.body_size, Some(18));
    }

    #[test]
    fn out_of_range_metrics_are_dropped_with_a_reason() {
        let dark = vars(&[("--font-size-md", "200px")]);
        let out = convert(&BTreeMap::new(), &dark);
        assert!(out.theme.style.body_size.is_none());
        let dropped = out
            .dropped
            .iter()
            .find(|d| d.target == "style.body_size")
            .expect("expected a reported drop");
        assert!(
            dropped.reason.contains("10..=28"),
            "got {:?}",
            dropped.reason
        );
    }

    #[test]
    fn spacing_pairs_are_assembled_per_axis() {
        let dark = vars(&[
            ("--spacing-8", "10px"),
            ("--spacing-4", "6px"),
            ("--spacing-12", "14px"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        let spacing = out.theme.style.item_spacing.expect("spacing should be set");
        assert_eq!(spacing, [10, 6]);
        let padding = out
            .theme
            .style
            .button_padding
            .expect("padding should be set");
        assert_eq!(padding, [14, 10]);
    }

    #[test]
    fn coverage_reflects_how_much_was_filled() {
        // `--text-normal` fills both `text` and `text_strong`, since a theme that sets one body colour
        // means the same value for high-emphasis text unless it says otherwise.
        let dark = vars(&[("--text-normal", "#ffffff"), ("--text-muted", "#888888")]);
        let out = convert(&BTreeMap::new(), &dark);
        assert_eq!(out.tokens_mapped, 3);
        assert!(
            out.coverage() > 0.0 && out.coverage() < 0.1,
            "got {}",
            out.coverage()
        );
    }

    #[test]
    fn an_empty_source_yields_an_empty_theme() {
        let out = convert(&BTreeMap::new(), &BTreeMap::new());
        assert!(out.theme.is_empty());
        assert_eq!(out.coverage(), 0.0);
        assert!(
            out.theme.validate().is_ok(),
            "an empty theme is still valid"
        );
    }

    #[test]
    fn converted_themes_always_validate() {
        let dark = vars(&[
            ("--background-primary", "#1e1f22"),
            ("--text-normal", "#dbdee1"),
            ("--font-size-md", "16px"),
            ("--radius-md", "6px"),
            ("--control-height", "48px"),
        ]);
        let out = convert(&BTreeMap::new(), &dark);
        assert!(
            out.theme.validate().is_ok(),
            "{:?}",
            out.theme.validate().err()
        );
    }

    #[test]
    fn numeric_value_parsing() {
        assert_eq!(numeric_value("16px", true), Some(16));
        assert_eq!(numeric_value("1.5rem", true), Some(24));
        assert_eq!(numeric_value("80%", false), Some(80));
        assert_eq!(numeric_value("32", false), Some(32));
        assert_eq!(numeric_value("calc(1px)", true), None);
    }

    #[test]
    fn text_on_matches_luminance() {
        assert_eq!(text_on(Rgba::rgb(255, 255, 255)), "#000000");
        assert_eq!(text_on(Rgba::rgb(0, 0, 0)), "#ffffff");
    }
}
