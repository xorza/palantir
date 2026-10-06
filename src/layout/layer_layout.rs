//! One layer's layout output.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::scene::tree::Tree;
use std::hash::Hasher as _;

/// Per-layer layout output — the SoA columns the encoder + hit-index
/// read after the layout pass. Intermediate scratch (desired sizes,
/// grid track state) lives on `LayoutScratch` directly. SoA columns
/// indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Debug, Default)]
pub(crate) struct LayerLayout {
    pub(crate) rect: Vec<Rect>,
    pub(crate) scroll_content: Vec<Size>,
    /// Flat per-frame buffer of shaped text runs. Leaf text appends during
    /// measure because it drives desired size; container text appends after
    /// arrange against its final padded width. Indexed via
    /// `text_spans[node]`.
    pub(crate) text_shapes: Vec<ShapedText>,
    /// Per-node `Span` into `text_shapes`. Empty span (`len: 0`) for
    /// nodes that didn't shape text. Same length as `rect`.
    pub(crate) text_spans: Vec<Span>,
    /// [`Self::rect_hash`], taken once the rects are final, so a run that
    /// keeps its output keeps it too.
    rect_hash: ContentHash,
}

impl LayerLayout {
    /// Destructured so a column added to `LayerLayout` cannot be left
    /// un-resized here — the per-node columns are indexed by `NodeId`
    /// without a bounds story of their own.
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        let Self {
            rect,
            scroll_content,
            text_shapes,
            text_spans,
            // Taken again by `hash_rects` once the rects are written.
            rect_hash: _,
        } = self;
        rect.clear();
        rect.resize(n, Rect::ZERO);
        scroll_content.clear();
        scroll_content.resize(n, Size::ZERO);
        // Flat, not per-node: spans index into it.
        text_shapes.clear();
        text_spans.clear();
        text_spans.resize(n, Span::default());
    }

    /// Summary of the arranged rects, for the cascade key
    /// (`CascadeKey::new`): a cascade built against other rects is
    /// neither skipped nor repaired in place.
    ///
    /// Hashed as raw bytes rather than through `FloatHash`'s visual
    /// quantisation on purpose: this gates a *cache-validity* decision,
    /// so it must be at least as strict as the exact element-wise
    /// comparison it replaces. Quantising would let a sub-quantum
    /// arrange shift retain a cascade built for the old rects. `Rect`
    /// is `Pod`, so the whole column hashes in one bulk write — the
    /// per-field form cost four hash rounds per rect instead of two.
    pub(crate) const fn rect_hash(&self) -> ContentHash {
        self.rect_hash
    }

    /// Take [`Self::rect_hash`] from the rects as they stand. Called by
    /// `LayoutEngine::run` once a layer's rects are final.
    pub(super) fn hash_rects(&mut self) {
        let mut h = Hasher::new();
        h.write_usize(self.rect.len());
        h.pod_slice(&self.rect);
        self.rect_hash = ContentHash(h.finish());
    }
}
