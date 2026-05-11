pub(crate) mod axis;
pub(crate) mod cache;
pub(crate) mod canvas;
pub(crate) mod grid;
pub(crate) mod intrinsic;
pub(crate) mod layoutengine;
pub(crate) mod scroll;
pub(crate) mod stack;
pub(crate) mod support;
pub(crate) mod types;
pub(crate) mod wrapstack;
pub(crate) mod zstack;

#[cfg(test)]
mod cross_driver_tests;

use crate::forest::tree::{Layer, Tree};
use crate::layout::types::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::text::TextCacheKey;
use crate::ui::cascade::Cascades;
use std::ops::{Index, IndexMut};
use strum::EnumCount as _;

/// Per-layer layout output — the SoA columns the encoder + hit-index
/// read after the layout pass. Intermediate scratch (desired sizes,
/// grid track hugs) lives on `LayoutScratch` directly. SoA columns
/// indexed by `NodeId.0`. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub(crate) struct LayerLayout {
    pub(crate) rect: Vec<Rect>,
    /// Flat per-frame buffer of shaped text runs. Grows during the
    /// measure pass: each `ShapeRecord::Text` on each leaf appends one
    /// entry. Indexed via `text_spans[node]`. Mirrors `tree.shapes` +
    /// `records.shape_span()` for the layout-derived counterpart.
    pub(crate) text_shapes: Vec<ShapedText>,
    /// Per-node `Span` into `text_shapes`. Empty span (`len: 0`) for
    /// nodes that didn't shape text. Same length as `rect`.
    pub(crate) text_spans: Vec<Span>,
}

/// Per-frame layout output across all layers. Wraps a fixed-size
/// `[LayerLayout; Layer::COUNT]` so callers index by `Layer` directly
/// (`result[Layer::Main]`) instead of casting through `usize`. Returned
/// by `LayoutEngine::run`; the encoder, cascade, hit-index, and tests
/// all read it.
#[derive(Default)]
pub(crate) struct Layout {
    pub(crate) layers: [LayerLayout; Layer::COUNT],
    /// Cascaded clip/disabled/invisible/transform per node + global
    /// hit index. Written by `CascadesEngine::run` in the paint phase
    /// and read by the encoder, input dispatch, and damage compute.
    pub(crate) cascades: Cascades,
}

impl Index<Layer> for Layout {
    type Output = LayerLayout;
    #[inline]
    fn index(&self, layer: Layer) -> &LayerLayout {
        &self.layers[layer as usize]
    }
}

impl IndexMut<Layer> for Layout {
    #[inline]
    fn index_mut(&mut self, layer: Layer) -> &mut LayerLayout {
        &mut self.layers[layer as usize]
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
        self.text_shapes.clear();
        self.text_spans.clear();
        self.text_spans.resize(n, Span::default());
    }
}
