//! A minimal element tree the native UI publishes so the CSS engine has something to select
//! against.
//!
//! The native client renders with an immediate-mode GUI and has no DOM. This module provides a
//! deliberately dumb parallel tree: enough structure for selectors (combinators, `:has()`,
//! `:nth-child()`, attribute matching) and nothing else. There is no layout, no box model, and
//! no events.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::classmap::Surface;

/// Build-time tree construction for shadow nodes.
///
/// [`ShadowNode`] uses `&'static [..]` slices so that widget code can declare stable trees as
/// `const` data with no allocation. This builder produces owned nodes for cases where the shape
/// is computed at runtime.
#[derive(Debug, Default, Clone)]
pub struct ShadowBuilder {
    tag: &'static str,
    id: Option<&'static str>,
    classes: Vec<&'static str>,
    data: BTreeMap<&'static str, String>,
    text: Option<String>,
    children: Vec<ShadowNode>,
}

impl ShadowBuilder {
    /// Starts a new node. `tag` defaults to `div`, matching how themes were written against
    /// Discord's markup.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tag: "div",
            ..Self::default()
        }
    }

    /// Overrides the tag name.
    #[must_use]
    pub fn tag(mut self, tag: &'static str) -> Self {
        self.tag = tag;
        self
    }

    /// Sets the `id` attribute, which themes target with `#id`.
    #[must_use]
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Adds a class name. Call repeatedly.
    #[must_use]
    pub fn class(mut self, class: &'static str) -> Self {
        self.classes.push(class);
        self
    }

    /// Sets a `data-*` attribute. The ecosystem's own stable escape hatch from hashed classes,
    /// via Vencord's `ThemeAttributes` and Equicord's `SurfaceClasses`.
    #[must_use]
    pub fn data(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.data.insert(key, value.into());
        self
    }

    /// Sets the node's text content.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Adds a child node.
    #[must_use]
    pub fn child(mut self, child: ShadowNode) -> Self {
        self.children.push(child);
        self
    }

    /// Finishes the node.
    #[must_use]
    pub fn build(self) -> ShadowNode {
        ShadowNode {
            tag: self.tag.to_owned(),
            id: self.id.map(str::to_owned),
            classes: self.classes.into_iter().map(str::to_owned).collect(),
            data: self
                .data
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            text: self.text,
            children: self.children,
        }
    }
}

/// One element in the shadow tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowNode {
    /// Element name, e.g. `div`, `span`, `input`.
    #[serde(default = "default_tag")]
    pub tag: String,
    /// The `id` attribute, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Class names on this element. The native UI emits both the stable name and, optionally, a
    /// hashed alias so partially-translated themes still match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// `data-*` attributes, plus any other attributes themes select on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, String>,
    /// Text content, used by `::before { content: "..." }` style rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Child nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ShadowNode>,
}

/// Default tag for a [`ShadowNode`], matching how themes were written against Discord's markup.
fn default_tag() -> String {
    "div".to_owned()
}

impl ShadowNode {
    /// Starts building a node.
    #[must_use]
    pub fn builder() -> ShadowBuilder {
        ShadowBuilder::new()
    }

    /// Whether this element carries the given class.
    #[must_use]
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// Value of the `id` attribute.
    #[must_use]
    pub fn id_value(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Value of any attribute, including `id`, `class`, and everything in `data`.
    ///
    /// Returns `None` when the element lacks the attribute, which lets callers distinguish "no
    /// `class` attribute" from an empty one. The `class` case borrows because the joined list is
    /// computed on demand rather than stored.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<Cow<'_, str>> {
        match name {
            "id" => self.id.as_deref().map(Cow::Borrowed),
            "class" => {
                if self.classes.is_empty() {
                    None
                } else {
                    Some(Cow::Owned(self.classes.join(" ")))
                }
            }
            _ => self.data.get(name).map(|v| Cow::Borrowed(v.as_str())),
        }
    }

    /// Text content.
    #[must_use]
    pub fn text_value(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Depth-first iteration over this node and all descendants.
    pub fn descendants(&self) -> impl Iterator<Item = &ShadowNode> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            for child in node.children.iter().rev() {
                stack.push(child);
            }
            Some(node)
        })
    }

    /// Direct children.
    #[must_use]
    pub fn children(&self) -> &[ShadowNode] {
        &self.children
    }

    /// Index of `node` among its parent's element children, 1-based, as CSS `:nth-child` expects.
    /// Used by themes that locate controls positionally, e.g. to find Settings tabs by order.
    #[must_use]
    pub fn index_among_siblings(node: &ShadowNode, parent: &ShadowNode) -> Option<usize> {
        parent
            .children
            .iter()
            .position(|c| std::ptr::eq(c, node))
            .map(|i| i + 1)
    }
}

/// A path identifying a node in the shadow tree, used to report which regions a theme matched.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodePath(Vec<usize>);

impl NodePath {
    /// Builds a path from root-relative child indices.
    #[must_use]
    pub fn new(indices: Vec<usize>) -> Self {
        Self(indices)
    }

    /// The indices, outermost first.
    #[must_use]
    pub fn indices(&self) -> &[usize] {
        &self.0
    }

    /// Depth of the node (root is 0).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

/// The root of the shadow tree plus its top-level regions.
#[derive(Debug, Clone, Default)]
pub struct ShadowTree {
    root: ShadowNode,
}

impl ShadowTree {
    /// Wraps a root node.
    #[must_use]
    pub fn new(root: ShadowNode) -> Self {
        Self { root }
    }

    /// The root element.
    #[must_use]
    pub fn root(&self) -> &ShadowNode {
        &self.root
    }

    /// Resolves a [`NodePath`] against this tree.
    #[must_use]
    pub fn node_at(&self, path: &NodePath) -> Option<&ShadowNode> {
        let mut node = &self.root;
        for &index in path.indices() {
            node = node.children.get(index)?;
        }
        Some(node)
    }

    /// Every node in the tree, paired with its path.
    #[must_use]
    pub fn walk(&self) -> Vec<(NodePath, &ShadowNode)> {
        let mut out = Vec::new();
        collect(&self.root, &mut Vec::new(), &mut out);
        out
    }

    /// Counts how many nodes fall in each surface, judged by nearest ancestor with a
    /// `data-dcrs-surface` marker. Used by the porting tool to report a theme's coverage.
    #[must_use]
    pub fn surface_histogram(&self) -> std::collections::BTreeMap<Surface, usize> {
        let mut hist = std::collections::BTreeMap::new();
        for (_, node) in self.walk() {
            let Some(marker) = node.attr("data-dcrs-surface") else {
                continue;
            };
            if let Some(surface) = Surface::parse(&marker) {
                *hist.entry(surface).or_insert(0) += 1;
            }
        }
        hist
    }
}

fn collect<'a>(
    node: &'a ShadowNode,
    path: &mut Vec<usize>,
    out: &mut Vec<(NodePath, &'a ShadowNode)>,
) {
    out.push((NodePath(path.clone()), node));
    for (i, child) in node.children.iter().enumerate() {
        path.push(i);
        collect(child, path, out);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> ShadowTree {
        // <div class="theme-dark">
        //   <div data-dcrs-surface="channel-list">
        //     <div class="channel"><div class="label">general</div></div>
        //     <div class="channel"><div class="label">random</div></div>
        //   </div>
        //   <div data-dcrs-surface="chat">
        //     <ul data-list-id="chat-messages">
        //       <li data-is-self="false"><span class="messageContent-1c07e6">hi</span></li>
        //       <li data-is-self="true"><span class="messageContent-1c07e6">hey</span></li>
        //     </ul>
        //   </div>
        // </div>
        let channel = |label: &'static str| {
            ShadowNode::builder()
                .class("channel")
                .child(ShadowNode::builder().class("label").text(label).build())
                .build()
        };
        let message = |is_self: &'static str| {
            ShadowNode::builder()
                .tag("li")
                .data("data-is-self", is_self)
                .child(
                    ShadowNode::builder()
                        .class("messageContent-1c07e6")
                        .text("body")
                        .build(),
                )
                .build()
        };
        ShadowTree::new(
            ShadowNode::builder()
                .class("theme-dark")
                .child(
                    ShadowNode::builder()
                        .data("data-dcrs-surface", "channel-list")
                        .child(channel("general"))
                        .child(channel("random"))
                        .build(),
                )
                .child(
                    ShadowNode::builder()
                        .data("data-dcrs-surface", "chat")
                        .child(
                            ShadowNode::builder()
                                .tag("ul")
                                .data("data-list-id", "chat-messages")
                                .child(message("false"))
                                .child(message("true"))
                                .build(),
                        )
                        .build(),
                )
                .build(),
        )
    }

    #[test]
    fn builder_produces_expected_attributes() {
        let node = ShadowNode::builder()
            .tag("input")
            .id("composer")
            .class("chat-input")
            .data("data-channel-id", "123")
            .text("draft")
            .build();
        assert_eq!(node.tag, "input");
        assert_eq!(node.id_value(), Some("composer"));
        assert!(node.has_class("chat-input"));
        assert_eq!(node.attr("data-channel-id").as_deref(), Some("123"));
        assert_eq!(node.attr("class").as_deref(), Some("chat-input"));
        assert_eq!(node.text_value(), Some("draft"));
        assert_eq!(node.attr("missing"), None);
    }

    #[test]
    fn descendants_visits_every_node() {
        let tree = tree();
        let root = tree.root();
        // 1 root + 1 channel-list + 2 channels + 2 labels + 1 chat + 1 ul + 2 li + 2 spans = 12
        assert_eq!(root.descendants().count(), 12);
    }

    #[test]
    fn node_at_resolves_paths() {
        let tree = tree();
        let found = tree.walk();
        for (path, node) in &found {
            assert_eq!(
                tree.node_at(path),
                Some(*node),
                "path {:?} did not round-trip",
                path.indices()
            );
        }
        assert_eq!(tree.node_at(&NodePath::new(vec![99])), None);
    }

    #[test]
    fn nth_child_index_is_one_based() {
        let tree = tree();
        let (_, channel_list) = tree
            .walk()
            .into_iter()
            .find(|(_, n)| n.attr("data-dcrs-surface").as_deref() == Some("channel-list"))
            .unwrap();
        let first = &channel_list.children[0];
        let second = &channel_list.children[1];
        assert_eq!(
            ShadowNode::index_among_siblings(first, channel_list),
            Some(1)
        );
        assert_eq!(
            ShadowNode::index_among_siblings(second, channel_list),
            Some(2)
        );
    }

    #[test]
    fn surface_histogram_counts_marked_regions() {
        let hist = tree().surface_histogram();
        assert_eq!(hist[&Surface::ChannelList], 1);
        assert_eq!(hist[&Surface::Chat], 1);
        assert!(!hist.contains_key(&Surface::MemberList));
    }

    #[test]
    fn shadow_nodes_serialize_round_trip() {
        let tree = tree();
        let json = serde_json::to_string(tree.root()).unwrap();
        let back: ShadowNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, *tree.root());
    }
}
