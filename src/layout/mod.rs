pub(crate) mod axis;
pub(crate) mod cache;
pub(crate) mod canvas;
pub(crate) mod engine;
pub(crate) mod grid;
pub(crate) mod intrinsic;
pub(crate) mod scroll;
pub(crate) mod stack;
pub(crate) mod support;
pub(crate) mod types;
pub(crate) mod wrapstack;
pub(crate) mod zstack;

#[cfg(test)]
mod cross_driver_tests;

use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::layer::Layer;
use crate::scene::layer::PerLayer;
use crate::scene::tree::Tree;
use crate::text::TextCacheKey;
use std::ops::{Index, IndexMut};

/// Per-layer layout output — the SoA columns the encoder + hit-index
/// read after the layout pass. Intermediate scratch (desired sizes,
/// grid track hugs) lives on `LayoutScratch` directly. SoA columns
/// indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
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
}

/// Per-frame layout output across all layers. Callers index by
/// `Layer` directly (`result[Layer::Main]`) — see [`PerLayer`].
/// Returned by `LayoutEngine::run`; the encoder, cascade, hit-index,
/// and tests all read it. (The cascade pass's own output lives on
/// `Ui::cascades` — this struct is purely the layout pass's product.)
#[derive(Default)]
pub(crate) struct Layout {
    pub(crate) layers: PerLayer<LayerLayout>,
}

impl Index<Layer> for Layout {
    type Output = LayerLayout;
    #[inline]
    fn index(&self, layer: Layer) -> &LayerLayout {
        &self.layers[layer]
    }
}

impl IndexMut<Layer> for Layout {
    #[inline]
    fn index_mut(&mut self, layer: Layer) -> &mut LayerLayout {
        &mut self.layers[layer]
    }
}

/// Result of shaping one `ShapeRecord::Text` during the measure pass. `Tree`
/// records only the authoring inputs; this is the layout-side derived state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShapedText {
    pub measured: Size,
    pub key: TextCacheKey,
}

impl LayerLayout {
    pub(crate) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.scroll_content.clear();
        self.scroll_content.resize(n, Size::ZERO);
        self.text_shapes.clear();
        self.text_spans.clear();
        self.text_spans.resize(n, Span::default());
    }
}
