use crate::layout::AvailableKey;
use crate::primitives::{Rect, Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};
use std::ops::Range;

/// Per-frame layout *output* — strictly the state read after the layout
/// pass by the encoder + hit-index. Intermediate scratch (desired
/// sizes, grid track hugs) lives on `LayoutScratch` directly. SoA
/// columns indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub struct LayoutResult {
    rect: Vec<Rect>,
    /// Per-node shape result for `Shape::Text` leaves. `None` for any
    /// node the layout pass didn't shape text for.
    text_shapes: Vec<Option<ShapedText>>,
    /// Per-node quantized `available` size, the dimensional half of
    /// the cross-frame cache key. Written on every measure entry,
    /// restored from a snapshot on cache-hit subtrees. Read by the
    /// encode cache (and any other consumer keyed on the same
    /// `(subtree_hash, available_q)` shape as `MeasureCache`).
    pub(in crate::layout) available_q: Vec<AvailableKey>,
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
        self.available_q.resize(n, AvailableKey::UNSET);
    }

    #[inline]
    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    /// Per-node quantized `available` size last passed to this node's
    /// measure. `None` when this node was never visited by the current
    /// frame's layout `run` (collapsed root, empty frame, or — defensively
    /// — any future caller that reads a slot before `measure` writes it).
    /// Read by the encode cache.
    #[inline]
    pub fn available_q(&self, id: NodeId) -> Option<AvailableKey> {
        let v = self.available_q[id.index()];
        if v == AvailableKey::UNSET {
            None
        } else {
            Some(v)
        }
    }

    #[inline]
    pub(in crate::layout) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    #[inline]
    pub fn text_shape(&self, id: NodeId) -> Option<ShapedText> {
        self.text_shapes[id.index()]
    }

    #[inline]
    pub(in crate::layout) fn set_text_shape(&mut self, id: NodeId, s: ShapedText) {
        self.text_shapes[id.index()] = Some(s);
    }

    #[inline]
    pub(in crate::layout) fn text_shapes_slice(
        &self,
        range: Range<usize>,
    ) -> &[Option<ShapedText>] {
        &self.text_shapes[range]
    }

    #[inline]
    pub(in crate::layout) fn restore_text_shapes(
        &mut self,
        start: usize,
        src: &[Option<ShapedText>],
    ) {
        let end = start + src.len();
        self.text_shapes[start..end].copy_from_slice(src);
    }
}
