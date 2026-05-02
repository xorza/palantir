use crate::primitives::{Rect, Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};

/// Per-frame layout *output* — strictly the state read after the layout
/// pass by the encoder + hit-index. Intermediate scratch (desired sizes,
/// grid track hugs, future intrinsics) lives on `LayoutEngine` directly.
/// Capacity is reused across frames via `resize_for`.
#[derive(Default)]
pub struct LayoutResult {
    rect: Vec<Rect>,
    /// Per-node shape result for `Shape::Text` leaves. `None` for any
    /// node the layout pass didn't shape text for. SoA column indexed by
    /// `NodeId.0`, matching the rest of the engine.
    text_shapes: Vec<Option<ShapedText>>,
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
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
        self.text_shapes.resize(n, None);
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    pub(super) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    pub fn text_shape(&self, id: NodeId) -> Option<ShapedText> {
        self.text_shapes[id.index()]
    }

    pub(super) fn set_text_shape(&mut self, id: NodeId, s: ShapedText) {
        self.text_shapes[id.index()] = Some(s);
    }
}
