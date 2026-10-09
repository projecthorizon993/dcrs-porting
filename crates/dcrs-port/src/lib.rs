//! Analysis and conversion for Discord themes and mods.
//!
//! The binary is a thin wrapper over this library. Everything the commands do lives here so the same
//! logic can be tested, embedded, and eventually driven from a porting pipeline without shelling out.
//!
//! # Commands
//!
//! - [`theme`] — what a theme's CSS is made of, and how portable each part is.
//! - [`effects`] — what a plugin does, independent of the mechanism it uses.
//! - [`convert`] — turn a theme into a native Serein `.serein-extension` package, and account for
//!   everything that did not survive.
//!
//! [`analyze`] backs the `plugin` subcommand.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]
// Report rendering is far clearer as a sequence of `push_str(&format!(..))` calls than as one deeply
// nested `write!` chain, and these three modules all render reports.
#![allow(clippy::format_push_string)]

pub mod analyze;
pub mod convert;
pub mod effects;
pub mod theme;
