use crate::layout::cache::AvailableKey;
use crate::primitives::{Rect, Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};

/// Per-frame layout *output* — strictly the state read after the layout
/// pass by the encoder + hit-index. Intermediate scratch (desired sizes,
/// grid track hugs, intrinsic cache) lives on `LayoutEngine` directly.
/// Capacity is reused across frames via `resize_for`.
#[derive(Default)]
pub struct LayoutResult {
    rect: Vec<Rect>,
    /// Per-node shape result for `Shape::Text` leaves. `None` for any
    /// node the layout pass didn't shape text for. SoA column indexed by
    /// `NodeId.0`, matching the rest of the engine.
    text_shapes: Vec<Option<ShapedText>>,
    /// Per-node quantized `available` size — the dimensional half of
    /// the cross-frame cache key. Written by `LayoutEngine::measure`
    /// on every entry, restored by the measure cache on subtree-skip
    /// hits so the column stays correct even when the descendant
    /// recursion is short-circuited. Read by the encode cache (and
    /// any other consumer that needs the same key).
    available_q: Vec<AvailableKey>,
}

/// Result of shaping one `Shape::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub struct ShapedText {
    pub measured: Size,
    pub key: TextCacheKey,
}

impl LayoutResult {
    pub(in crate::layout) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
        self.text_shapes.resize(n, None);
        self.available_q.clear();
        self.available_q.resize(n, AvailableKey::ZERO);
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    pub(in crate::layout) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    pub fn text_shape(&self, id: NodeId) -> Option<ShapedText> {
        self.text_shapes[id.index()]
    }

    pub(in crate::layout) fn set_text_shape(&mut self, id: NodeId, s: ShapedText) {
        self.text_shapes[id.index()] = Some(s);
    }

    /// Read-only slice over `text_shapes` for snapshotting a whole
    /// subtree into the measure cache.
    pub(in crate::layout) fn text_shapes_slice(
        &self,
        start: usize,
        end: usize,
    ) -> &[Option<ShapedText>] {
        &self.text_shapes[start..end]
    }

    /// Bulk write a contiguous range of `text_shapes` (for restoring a
    /// subtree from the measure cache). `src.len()` must equal the
    /// destination range.
    pub(in crate::layout) fn restore_text_shapes(
        &mut self,
        start: usize,
        src: &[Option<ShapedText>],
    ) {
        let end = start + src.len();
        self.text_shapes[start..end].copy_from_slice(src);
    }

    /// Quantized `available` size last passed to this node's measure.
    /// Read by the encode cache (and any other consumer keyed on the
    /// same `(subtree_hash, available_q)` shape as `MeasureCache`).
    pub fn available_q(&self, id: NodeId) -> AvailableKey {
        self.available_q[id.index()]
    }

    pub(in crate::layout) fn set_available_q(&mut self, id: NodeId, v: AvailableKey) {
        self.available_q[id.index()] = v;
    }

    pub(in crate::layout) fn available_q_slice(&self, start: usize, end: usize) -> &[AvailableKey] {
        &self.available_q[start..end]
    }

    pub(in crate::layout) fn restore_available_q(&mut self, start: usize, src: &[AvailableKey]) {
        let end = start + src.len();
        self.available_q[start..end].copy_from_slice(src);
    }
}
