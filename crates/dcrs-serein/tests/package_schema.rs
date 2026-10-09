//! The package shape is checked against packages Serein actually ships.
//!
//! `docs/theme-api.md` documents the schema in prose, and prose drifts. These fixtures are verbatim
//! files from `extensions/themes` in ViceVerse-cz/Serein, so a field renamed or added upstream fails
//! here rather than producing a package that silently refuses to install.
//!
//! Only colour-and-metric themes are vendored. The two official packages that carry a background
//! are 6 MB each because the image bytes are a JSON array, which is too much to keep in a repo for
//! no extra coverage.

use dcrs_serein::{Package, Theme};

/// Every fixture must round-trip through this crate's own types.
fn parse(name: &str) -> Package {
    let path = format!("tests/fixtures/{name}");
    let json =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{name} did not parse: {e}"))
}

#[test]
fn official_packages_parse() {
    for name in ["golden.serein-extension", "ocean.serein-extension"] {
        let package = parse(name);
        assert_eq!(
            package.manifest.api_version,
            dcrs_serein::API_VERSION,
            "{name}"
        );
        assert_eq!(package.manifest.kind, dcrs_serein::Kind::Theme, "{name}");
        assert!(
            package.wasm.is_empty(),
            "{name}: a theme must carry no Wasm module"
        );
        assert!(package.manifest.capabilities.is_empty(), "{name}");
        assert!(package.manifest.actions.is_empty(), "{name}");
        assert!(
            package.background_image.is_empty(),
            "{name}: fixture is colour-only"
        );
    }
}

#[test]
fn official_packages_satisfy_our_own_validation() {
    // The same `validate` our builder applies before emitting. If a real shipped theme failed it,
    // we would be rejecting themes Serein itself accepts.
    for name in ["golden.serein-extension", "ocean.serein-extension"] {
        let package = parse(name);
        if let Err(errors) = package.theme.validate() {
            panic!("{name} would be rejected by our own validator: {errors:?}");
        }
    }
}

#[test]
fn a_full_palette_covers_every_token() {
    let package = parse("golden.serein-extension");
    // Golden sets all 18 tokens in both appearances, which is the upper bound the host enforces.
    assert_eq!(package.theme.dark.colors.len(), 18);
    assert_eq!(package.theme.light.colors.len(), 18);
    assert_eq!(package.token_count(), 36);
}

#[test]
fn a_full_palette_survives_a_serialization_round_trip() {
    let package = parse("golden.serein-extension");
    let json = package.to_json().expect("should serialize");
    let again: Package = serde_json::from_str(&json).expect("should re-parse");
    assert_eq!(package, again);
}

#[test]
fn an_unknown_field_is_refused_rather_than_ignored() {
    // `deny_unknown_fields` is what stops us emitting a key the host would reject outright. A
    // package that parses here but not in Serein is the worst outcome this crate could produce.
    let json = r#"{"manifest":{"api_version":1,"id":"t","name":"T","version":"1.0.0",
        "author":"a","license":"MIT","source":"","kind":"theme"},"theme":{"nope":{}}}"#;
    assert!(serde_json::from_str::<Package>(json).is_err());
}

#[test]
fn a_theme_object_is_required_by_this_crate() {
    // The host parses `theme` as optional and then rejects a `theme` package without one. This
    // crate makes it a required field instead: there is no case where we emit a theme manifest
    // without a theme, so representing that state would only let a bug through to the install step.
    let json = r#"{"manifest":{"api_version":1,"id":"t","name":"T","version":"1.0.0",
        "author":"a","license":"MIT","source":"","kind":"theme"}}"#;
    assert!(
        serde_json::from_str::<Package>(json).is_err(),
        "a theme package needs a theme"
    );
}

#[test]
fn an_empty_theme_is_still_a_valid_theme_object() {
    let theme = Theme::default();
    assert!(theme.validate().is_ok());
    assert_eq!(theme.token_count(), 0);
}
