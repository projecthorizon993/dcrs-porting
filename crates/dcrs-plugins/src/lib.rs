//! Native plugin ABI for ported Discord mods.
//!
//! A ported plugin is a Rust crate implementing [`Plugin`]. The manifest mirrors Vencord's
//! `PluginDef` field-for-field so that a port stays recognizable against the original, and so
//! that the porting tool can map a source plugin onto this shape mechanically.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod manifest;
pub mod registry;

pub use manifest::{Author, Manifest, OptionKind, PluginTag, RenderSlot, StartAt};
pub use registry::{Host, Plugin, PluginError, PluginId};

/// A minimal widget description produced by a plugin and drawn by the host UI.
///
/// Plugins return these instead of touching a UI crate directly, which keeps the ABI stable and
/// means a plugin can be unit-tested without a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiNode {
    /// Node kind, e.g. `text`, `button`, `spacer`, `image`.
    pub kind: String,
    /// Text payload, for text and button nodes.
    pub text: Option<String>,
    /// Extra style keys, so plugins can pick up theme classes.
    pub classes: Vec<String>,
}

impl UiNode {
    /// A text node.
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            kind: "text".to_owned(),
            text: Some(content.into()),
            classes: Vec::new(),
        }
    }

    /// A button node.
    #[must_use]
    pub fn button(label: impl Into<String>) -> Self {
        Self {
            kind: "button".to_owned(),
            text: Some(label.into()),
            classes: Vec::new(),
        }
    }

    /// A vertical spacer.
    #[must_use]
    pub fn spacer() -> Self {
        Self {
            kind: "spacer".to_owned(),
            text: None,
            classes: Vec::new(),
        }
    }

    /// Attaches a class name.
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.classes.push(class.into());
        self
    }
}
