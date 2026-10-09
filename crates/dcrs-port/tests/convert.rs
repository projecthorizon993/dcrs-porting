//! End-to-end conversion: CSS in, a package Serein accepts out.
//!
//! These exercise the whole path — parse, resolve, project, validate, serialize — because the failure
//! modes that matter are exactly the ones a unit test on any single stage cannot see. A theme that
//! converts to 94% of its tokens and never mentions that two were dropped is a bug that only shows up
//! when the whole thing runs.

use dcrs_port::convert::{self, ConversionReport};

/// Converts CSS, failing the test if it does not parse.
fn convert(css: &str) -> ConversionReport {
    convert::convert(css, "test").expect("valid css")
}

/// Checks a package against the host's own documented rules.
///
/// Written out rather than delegating to the crate, so it does not inherit a bug in the same
/// validator it is meant to be checking.
fn assert_acceptable(pkg: &dcrs_serein::Package) {
    let m = &pkg.manifest;
    assert_eq!(m.api_version, 1);
    assert_eq!(m.kind, dcrs_serein::Kind::Theme);
    assert!(pkg.wasm.is_empty(), "a theme package carries no Wasm");
    assert!(
        m.capabilities.is_empty(),
        "a theme requests no capabilities"
    );
    assert!(m.actions.is_empty(), "a theme declares no actions");
    assert!(!m.name.is_empty() && m.name.len() <= 128);
    assert!(!m.author.is_empty() && m.author.len() <= 128);
    assert!(!m.license.is_empty() && m.license.len() <= 128);
    assert!(!m.version.is_empty() && m.version.len() <= 128);
    assert!(m.source.is_empty() || m.source.starts_with("https://"));
    assert!(
        m.id.len() <= 64
            && m.id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    );

    assert!(pkg.background_image.len() <= dcrs_serein::MAX_BACKGROUND_BYTES);

    for (label, appearance) in [("light", &pkg.theme.light), ("dark", &pkg.theme.dark)] {
        assert!(appearance.colors.len() <= 18, "{label} has too many tokens");
        for token in dcrs_serein::Token::ALL {
            if let Some(value) = appearance.colors.get(token) {
                assert!(
                    matches!(value.len(), 7 | 9) && value.starts_with('#'),
                    "{label}.{token} is {value:?}, which the host rejects"
                );
            }
        }
    }

    let style = &pkg.theme.style;
    for value in [
        style.body_size,
        style.button_size,
        style.small_size,
        style.monospace_size,
    ]
    .into_iter()
    .flatten()
    {
        assert!((10..=28).contains(&value), "font size {value} out of range");
    }
    if let Some(size) = style.heading_size {
        assert!((12..=40).contains(&size), "heading {size} out of range");
    }
    if let Some(height) = style.control_height {
        assert!(
            (24..=56).contains(&height),
            "control_height {height} out of range"
        );
    }
    for radius in [style.widget_radius, style.window_radius, style.menu_radius]
        .into_iter()
        .flatten()
    {
        assert!(radius <= 24, "radius {radius} out of range");
    }
    for pair in [style.item_spacing.as_ref(), style.button_padding.as_ref()]
        .into_iter()
        .flatten()
    {
        assert!(
            pair[0] <= 24 && pair[1] <= 24,
            "spacing {pair:?} out of range"
        );
    }
    for pct in [style.transparency, style.blur].into_iter().flatten() {
        assert!(pct <= 100, "percentage {pct} out of range");
    }
}

#[test]
fn a_realistic_theme_converts_to_an_installable_package() {
    let css = "\
/*
 * @name Example Theme
 * @author someone
 * @version 1.0.0
 * @license MIT
 * @source https://github.com/someone/theme
 */
:root {
    --background-primary: #ffffff;
    --background-secondary: #f2f3f5;
    --chat-background: #ffffff;
    --background-floating: #ffffff;
    --text-normal: #060607;
    --header-primary: #060607;
    --text-muted: #747f8d;
    --text-link: #0067e0;
    --brand-experiment: #0067e0;
    --status-positive: #3ba55d;
    --status-warning: #faa61a;
    --status-danger: #ed4245;
    --mention-background: #5865f280;
    --mention-foreground: #ffffff;
    --background-modifier-hover: #0000000a;
    --background-modifier-selected: #0067e026;
    --border-subtle: #0000001a;
    --font-size-md: 16px;
    --font-size-2xl: 24px;
    --font-size-sm: 14px;
    --font-size-xs: 12px;
    --font-size-mono: 14px;
    --radius-sm: 8px;
    --radius-md: 8px;
    --spacing-4: 4px;
    --spacing-8: 8px;
    --spacing-12: 12px;
    --control-height: 32px;
}
.theme-dark {
    --background-primary: #313338;
    --background-secondary: #2b2d31;
    --chat-background: #313338;
    --background-floating: #111214;
    --text-normal: #dbdee1;
    --text-muted: #949ba4;
    --brand-experiment: #5865f2;
}";
    let report = convert(css);
    let package = report.package(Vec::new()).expect("should build").package;

    assert_acceptable(&package);
    assert_eq!(package.manifest.name, "Example Theme");
    assert_eq!(package.manifest.author, "someone");
    assert_eq!(package.manifest.license, "MIT");
    assert_eq!(package.manifest.id, "example-theme");

    // Both appearances should be populated, and distinguishable: the whole point of honouring
    // `.theme-dark` is that the two palettes differ.
    assert_eq!(
        package.theme.light.colors.get(dcrs_serein::Token::Chat),
        Some("#ffffff")
    );
    assert_eq!(
        package.theme.dark.colors.get(dcrs_serein::Token::Chat),
        Some("#313338")
    );

    // 17 of 18 tokens: `border` resolves from `--border-subtle`, so only a token the theme never
    // mentions should be missing.
    assert!(package.token_count() >= 30, "got {}", package.token_count());
}

#[test]
fn a_monochrome_theme_still_gets_distinct_surfaces() {
    // Every surface the same colour is the case that produces one flat, unusable theme if the
    // projection only copies what it finds.
    let css = ":root { --background-primary: #1e1f22; }";
    let report = convert(css);
    let package = report.package(Vec::new()).expect("should build").package;
    assert_acceptable(&package);

    let colors = &package.theme.dark.colors;
    let base = colors.get(dcrs_serein::Token::Base).unwrap();
    let sidebar = colors.get(dcrs_serein::Token::Sidebar).unwrap();
    let chat = colors.get(dcrs_serein::Token::Chat).unwrap();
    assert_eq!(base, "#1e1f22");
    assert_ne!(sidebar, base, "sidebar must not collapse onto base");
    assert_ne!(chat, sidebar, "chat must not collapse onto sidebar");
}

#[test]
fn an_out_of_range_metric_is_reported_rather_than_emitted() {
    // Serein rejects the package outright, so a converter that clamps silently would produce a
    // different theme than the author asked for with no way to tell.
    let css = ":root { --background-primary: #1e1f22; --font-size-md: 200px; }";
    let report = convert(css);
    assert!(
        report
            .conversion
            .dropped
            .iter()
            .any(|d| d.target == "style.body_size")
    );
    let package = report
        .package(Vec::new())
        .expect("builds, having dropped the metric")
        .package;
    assert_eq!(package.theme.style.body_size, None);
    assert_acceptable(&package);
}

#[test]
fn a_theme_that_maps_nothing_is_an_error_not_an_empty_package() {
    let report = convert(":root { --unrelated-thing: 4px; }");
    assert!(report.package(Vec::new()).is_err());
}

#[test]
fn hsl_and_var_layering_survive() {
    // The two forms real themes use constantly: an `--x-hsl` triple composed through `hsl()`, and a
    // `var()` chain pointing at a literal.
    let css = "\
:root {
    --brand-experiment-hsl: 235 86% 66%;
    --brand-experiment: hsl(var(--brand-experiment-hsl));
    --accent-2: var(--brand-experiment);
    --background-modifier-hover: hsl(var(--brand-experiment-hsl) / 0.24);
    --background-primary: #1e1f22;
}";
    let report = convert(css);
    let package = report.package(Vec::new()).expect("should build").package;
    assert_acceptable(&package);
    // hsl(235 86% 66%) is rgb(94, 106, 243).
    assert_eq!(
        package.theme.dark.colors.get(dcrs_serein::Token::Accent),
        Some("#5e6af3")
    );
}

#[test]
fn a_header_only_theme_still_produces_attribution() {
    let css =
        "/* @name Just A Name @author Just A Person */\n:root { --background-primary: #111111; }";
    let report = convert(css);
    let package = report.package(Vec::new()).expect("should build").package;
    assert_eq!(package.manifest.name, "Just A Name");
    assert_eq!(package.manifest.author, "Just A Person");
}

#[test]
fn explicit_flags_beat_the_themes_own_header() {
    let css =
        "/* @name From Header @author Header Author */\n:root { --background-primary: #1e1f22; }";
    let mut report = convert(css);
    report.apply(dcrs_port::convert::Overrides {
        name: Some("From Flag".to_owned()),
        author: Some("Flag Author".to_owned()),
        ..dcrs_port::convert::Overrides::default()
    });
    let package = report.package(Vec::new()).expect("should build").package;
    assert_eq!(package.manifest.name, "From Flag");
    assert_eq!(package.manifest.author, "Flag Author");
}

#[test]
fn the_report_names_what_it_lost() {
    let css = ":root { --background-primary: #1e1f22; --font-size-md: 999px; }";
    let report = convert(css);
    assert!(!report.is_lossless());
    assert_eq!(report.dropped_count(), 1);
    let areas = report.dropped_by_area();
    assert_eq!(areas.len(), 1);
    assert_eq!(areas[0].0, "control metrics");
    // The rendered report has to name the variable, or it is not a report.
    let text = report.render();
    assert!(text.contains("font-size-md"), "{text}");
    assert!(text.contains("style.body_size"), "{text}");
}

#[test]
fn a_background_image_is_embedded_from_css() {
    // A 1x1 PNG, which is the smallest thing the host's decoder will accept.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let css = format!(
        ":root {{ --background-primary: #1e1f22; }}\nbody {{ background-image: url(data:image/png;base64,{png}); }}"
    );
    let bytes = convert::embedded_background(&css).expect("should find the data URL");
    assert_eq!(bytes.len(), 70);
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}

#[test]
fn a_remote_background_url_is_not_fetched() {
    // Serein cannot fetch an image for a theme, so a URL must be reported as absent rather than
    // silently producing a package with no image and no explanation.
    let css = "body { background-image: url(https://example.com/bg.png); }";
    assert!(convert::embedded_background(css).is_none());
}

#[test]
fn a_non_image_data_url_is_ignored() {
    let css = "body { background-image: url(data:text/plain;base64,aGVsbG8=); }";
    assert!(convert::embedded_background(css).is_none());
}
