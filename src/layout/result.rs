use crate::forest::tree::{Layer, Tree};
use crate::layout::types::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextCacheKey;
use std::ops::{Index, IndexMut};
use strum::EnumCount as _;

/// Per-layer layout output — the SoA columns the encoder + hit-index
/// read after the layout pass. Intermediate scratch (desired sizes,
/// grid track hugs) lives on `LayoutScratch` directly. SoA columns
/// indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub(crate) struct LayerResult {
    pub(crate) rect: Vec<Rect>,
    /// Flat per-frame buffer of shaped text runs. Grows during the
    /// measure pass: each `Shape::Text` on each leaf appends one
    /// entry. Indexed via `text_spans[node]`. Mirrors `tree.shapes` +
    /// `records.shape_span()` for the layout-derived counterpart.
    pub(crate) text_shapes: Vec<ShapedText>,
    /// Per-node `Span` into `text_shapes`. Empty span (`len: 0`) for
    /// nodes that didn't shape text. Same length as `rect`.
    pub(crate) text_spans: Vec<Span>,
    /// Measured content extent for each `LayoutMode::Scroll{V, H, XY}`
    /// node. ScrollV stores `(max_w, sum_h + gap)`; ScrollH the mirror;
    /// ScrollXY stores `(max_w, max_h)`. Read by `Ui::end_frame` to
    /// refresh per-scroll-widget state rows for next frame's offset
    /// clamp. `Size::ZERO` for non-scroll nodes — they don't read this
    /// column.
    pub(crate) scroll_content: Vec<Size>,
}

/// Per-frame layout output across all layers. Wraps a fixed-size
/// `[LayerResult; Layer::COUNT]` so callers index by `Layer` directly
/// (`result[Layer::Main]`) instead of casting through `usize`. Returned
/// by `LayoutEngine::run`; the encoder, cascade, hit-index, and tests
/// all read it.
#[derive(Default)]
pub(crate) struct LayoutResult {
    pub(crate) layers: [LayerResult; Layer::COUNT],
}

impl Index<Layer> for LayoutResult {
    type Output = LayerResult;
    #[inline]
    fn index(&self, layer: Layer) -> &LayerResult {
        &self.layers[layer as usize]
    }
}

impl IndexMut<Layer> for LayoutResult {
    #[inline]
    fn index_mut(&mut self, layer: Layer) -> &mut LayerResult {
        &mut self.layers[layer as usize]
    }
}

/// Result of shaping one `Shape::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedText {
    pub measured: Size,
    pub key: TextCacheKey,
}

impl LayerResult {
    pub(crate) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_shapes.clear();
        self.text_spans.clear();
        self.text_spans.resize(n, Span::default());
        self.scroll_content.clear();
        self.scroll_content.resize(n, Size::ZERO);
    }
}
