use crate::layout::cache::{AVAIL_UNSET, AvailableKey};
use crate::layout::types::span::Span;
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
    /// Flat per-frame buffer of shaped text runs. Grows during the
    /// measure pass: each `Shape::Text` on each leaf appends one
    /// entry. Indexed via `text_spans[node]`. Mirrors `tree.shapes` +
    /// `records.shape_span()` for the layout-derived counterpart.
    pub(crate) text_shapes: Vec<ShapedText>,
    /// Per-node `Span` into `text_shapes`. Empty span (`len: 0`) for
    /// nodes that didn't shape text. Same length as `rect`.
    pub(crate) text_spans: Vec<Span>,
    /// Per-node quantized `available` size, the dimensional half of
    /// the cross-frame cache key. Written on every measure entry,
    /// restored from a snapshot on cache-hit subtrees. Read by the
    /// encode cache (and any other consumer keyed on the same
    /// `(subtree_hash, available_q)` shape as `MeasureCache`).
    pub(crate) available_q: Vec<AvailableKey>,
    /// Measured content extent for each `LayoutMode::Scroll{V, H, XY}`
    /// node. ScrollV stores `(max_w, sum_h + gap)`; ScrollH the mirror;
    /// ScrollXY stores `(max_w, max_h)`. Read by `Ui::end_frame` to
    /// refresh per-scroll-widget state rows for next frame's offset
    /// clamp. `Size::ZERO` for non-scroll nodes — they don't read this
    /// column.
    pub(crate) scroll_content: Vec<Size>,
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
        let n = tree.records.len();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
        self.text_spans.clear();
        self.text_spans.resize(n, Span::default());
        self.available_q.clear();
        self.available_q.resize(n, AVAIL_UNSET);
        self.scroll_content.clear();
        self.scroll_content.resize(n, Size::ZERO);
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
