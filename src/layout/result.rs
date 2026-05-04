use crate::layout::cache::{AVAIL_UNSET, AvailableKey};
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};

/// Per-frame layout *output* — strictly the state read after the layout
/// pass by the encoder + hit-index. Intermediate scratch (desired
/// sizes, grid track hugs) lives on `LayoutScratch` directly. SoA
/// columns indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub(crate) struct LayoutResult {
    pub(crate) rect: Vec<Rect>,
    /// Per-node shape result for `Shape::Text` leaves. `None` for any
    /// node the layout pass didn't shape text for.
    pub(crate) text_shapes: Vec<Option<ShapedText>>,
    /// Per-node quantized `available` size, the dimensional half of
    /// the cross-frame cache key. Written on every measure entry,
    /// restored from a snapshot on cache-hit subtrees. Read by the
    /// encode cache (and any other consumer keyed on the same
    /// `(subtree_hash, available_q)` shape as `MeasureCache`).
    pub(crate) available_q: Vec<AvailableKey>,
}

/// Result of shaping one `Shape::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedText {
    pub measured: Size,
    pub key: TextCacheKey,
}

impl LayoutResult {
    pub(crate) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
        self.text_shapes.resize(n, None);
        self.available_q.clear();
        self.available_q.resize(n, AVAIL_UNSET);
    }

    /// Per-node quantized `available` size last passed to this node's
    /// measure. `None` when this node was never visited by the current
    /// frame's layout `run` (collapsed root, empty frame, or — defensively
    /// — any future caller that reads a slot before `measure` writes it).
    /// Read by the encode cache.
    #[inline]
    pub(crate) fn available_q(&self, id: NodeId) -> Option<AvailableKey> {
        let v = self.available_q[id.index()];
        if v == AVAIL_UNSET { None } else { Some(v) }
    }
}
