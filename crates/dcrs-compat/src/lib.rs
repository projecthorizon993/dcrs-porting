//! Capability registry describing which third-party mod APIs a native client can replicate.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod registry;

pub use registry::{Class, Registry, RegistryError, Support, Surface, Verdict};
