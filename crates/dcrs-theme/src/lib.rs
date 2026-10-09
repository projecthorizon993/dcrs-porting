//! Tooling for porting BetterDiscord/Vencord themes to a native Discord client.
//!
//! Three pieces:
//!
//! - [`classmap`] — translate Discord's obfuscated class names to stable ones.
//! - [`css`] — parse the CSS subset themes ship, resolve variables, apply class-map translation.
//! - [`shadow`] — a minimal element tree so selectors have something to match against.
//!
//! # Scope
//!
//! This crate resolves *values*, not layout. Any theme rule that changes geometry (`width`,
//! `position`, `transform`, negative margins) is out of scope, because an immediate-mode GUI has
//! no CSS box model. See `docs/theme-tiers.md` for the tiering of what is and is not portable.
//!
//! # Example
//!
//! ```
//! use dcrs_theme::{ClassMap, Theme, ThemeKind};
//!
//! let theme = Theme::new(
//!     "Example",
//!     ":root { --background-primary: #313338; --accent: var(--background-primary); }",
//! )?;
//!
//! let vars = theme.resolve(Some(ThemeKind::Dark))?;
//! assert_eq!(vars.value("--accent"), Some("#313338"));
//!
//! // Hashed class names are translated through a scrape-maintained map.
//! let map = ClassMap::default();
//! assert!(theme.unmapped_classes(&map).is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod classmap;
pub mod color;
pub mod css;
pub mod shadow;
pub mod theme;

pub use classmap::{ClassMap, ClassMapError, Surface};
pub use color::{ColorError, Rgba};
pub use css::{ParseError, ResolveError, Stylesheet, ThemeKind, Variables};
pub use shadow::{ShadowNode, ShadowTree};
pub use theme::Theme;
