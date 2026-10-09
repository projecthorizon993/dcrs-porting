//! Converting BetterDiscord and Vencord themes into native Serein theme packages.
//!
//! Serein has no CSS injection surface and no need for one. Its theme API is a small declarative
//! object — eighteen colour tokens and fifteen control metrics per appearance — and a theme
//! package is JSON with no Wasm module and no capabilities.
//!
//! So this crate's job is projection, not translation: take a theme's several hundred Discord CSS
//! variables and land them on the tokens a native client actually reads. That is a smaller and more
//! tractable problem than emulating CSS, and it composes well with [`dcrs_theme`], which already
//! parses the source and resolves its `var()` graph.
//!
//! # Layout
//!
//! - [`theme`] — Serein's schema as typed Rust, with the documented bounds enforced.
//! - [`map`] — the projection tables from Discord variables onto tokens.
//! - [`package`] — assembly into a `.serein-extension` file.
//!
//! # Example
//!
//! ```no_run
//! use dcrs_serein::{Identity, from_theme};
//!
//! let css = ":root { --background-primary: #1e1f22; }
//!            .theme-dark { --text-normal: #dbdee1; --brand-experiment: #5865f2; }";
//! let sheet = dcrs_theme::Stylesheet::parse(css)?;
//!
//! let build = from_theme(Identity::from_name("Example", None), &sheet);
//! let package = build.package?;
//! println!("{}", package.to_json()?);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod map;
pub mod package;
pub mod theme;

pub use map::{Conversion, Dropped};
pub use package::{
    API_VERSION, Built, Header, Identity, Kind, MAX_BACKGROUND_BYTES, MAX_IMAGE_ALLOC,
    MAX_IMAGE_EDGE, MAX_IMAGE_PIXELS, MAX_PACKAGE_BYTES, Manifest, Package, PackageBuild,
    PackageError, convert, from_theme, from_theme_with_image, header_metadata, safe_id, valid_id,
    valid_source,
};
pub use theme::{
    Appearance, Background, Colors, Fit, METRICS, MetricRange, SectionOpacity, Style, Target,
    Theme, ThemeError, Token,
};
