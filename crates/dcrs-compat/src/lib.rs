//! Capability registry describing which third-party mod APIs a native client can replicate.
//!
//! Two axes, because one is not enough:
//!
//! - [`capability`] separates a mod's *mechanism* from its *effect*. A webpack patch cannot be
//!   ported; the boolean it flips is a field a native client already owns.
//! - [`media`] asks the harsher question of pixels and samples. A native client speaks every codec a
//!   mod could ask for, and still a media plugin has nothing to land on.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod capability;
pub mod media;
pub mod registry;

pub use capability::{Capability, GateRecipe};
pub use media::{MediaConcern, MediaReport};
pub use registry::{Class, Registry, RegistryError, Support, Surface, Verdict};
