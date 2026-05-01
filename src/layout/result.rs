use crate::primitives::{Rect, Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};
use std::collections::HashMap;

/// Per-frame layout *output*. Read after the layout pass by the encoder, hit
/// index, and any other consumer. Per-frame *scratch* (grid track hugs, etc.)
/// lives on `LayoutEngine.grid` instead. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub struct LayoutResult {
    desired: Vec<Size>,
    rect: Vec<Rect>,
    /// Per-node shape result for every `Shape::Text` the layout pass
    /// processed. Carries the measured size that fed the leaf's content
    /// hugging and the shaped-buffer `key` the encoder hands to the
    /// renderer. Keyed by `NodeId` because text widgets push exactly one
    /// `Shape::Text` per node.
    text_shapes: HashMap<NodeId, ShapedText>,
}

/// Result of shaping one `Shape::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub struct ShapedText {
    pub measured: Size,
    pub key: TextCacheKey,
}

impl LayoutResult {
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.desired[id.index()]
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    pub(super) fn set_desired(&mut self, id: NodeId, v: Size) {
        self.desired[id.index()] = v;
    }

    pub(super) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    pub fn text_shape(&self, id: NodeId) -> Option<&ShapedText> {
        self.text_shapes.get(&id)
    }

    pub(super) fn set_text_shape(&mut self, id: NodeId, s: ShapedText) {
        self.text_shapes.insert(id, s);
    }
}
