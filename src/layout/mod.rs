pub(crate) mod axis;
pub(crate) mod cache;
mod canvas;
pub(crate) mod engine;
pub(crate) mod grid;
pub(crate) mod intrinsic;
pub(crate) mod probe;
pub(crate) mod scroll;
pub(crate) mod scrollbars;
pub(crate) mod stack;
mod support;
pub(crate) mod types;
pub(crate) mod wrapstack;
pub(crate) mod zstack;

#[cfg(test)]
mod cross_driver_tests;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::layer::Layer;
use crate::scene::layer::PerLayer;
use crate::scene::seen_ids::Endpoint;
use crate::scene::tree::Tree;
use crate::text::key::TextShapeKey;
use std::hash::Hasher as _;
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
/// Filled in place by `LayoutEngine::run`, which takes it as `&mut` so the
/// buffers survive across frames; the encoder, cascade, hit-index,
/// and tests all read it afterwards. (The cascade pass's own output lives on
/// `Ui::cascade` — this struct is purely the layout pass's product.)
#[derive(Default)]
pub(crate) struct Layout {
    pub(crate) layers: PerLayer<LayerLayout>,
}

impl Layout {
    /// Measured content extent of the scroll viewport at `endpoint` —
    /// the size its bars express a ratio of, `ZERO` for any node that
    /// isn't a `LayoutMode::Scroll`.
    ///
    /// Takes an [`Endpoint`] because that is what
    /// [`Cascade::endpoint`](crate::scene::cascade::Cascade::endpoint)
    /// hands back: the two tables are keyed differently (by widget id,
    /// by node index) and only a caller holding both can bridge them.
    /// Naming each half keeps the bridge from being four raw indexes.
    #[inline]
    pub(crate) fn scroll_content(&self, endpoint: Endpoint) -> Size {
        self.layers[endpoint.layer].scroll_content[endpoint.node.idx()]
    }

    /// The node's arranged rect — pre-transform, unclipped, in world
    /// coords. Takes an [`Endpoint`] for the same reason
    /// [`Self::scroll_content`] does: this table is keyed by
    /// `(layer, node)` while its callers hold a `WidgetId`, and
    /// `Cascade` is what bridges the two.
    ///
    /// This is the single home of the arranged rects. `ResponseState`'s
    /// `layout_rect` reads through here rather than from a copy on the
    /// cascade's per-node row.
    #[inline]
    pub(crate) fn arranged_rect(&self, endpoint: Endpoint) -> Rect {
        self.layers[endpoint.layer].rect[endpoint.node.idx()]
    }
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
    pub(crate) measured: Size,
    pub(crate) key: TextShapeKey,
}

impl LayerLayout {
    fn resize_for(&mut self, tree: &Tree) {
        let n = tree.records.len();
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.scroll_content.clear();
        self.scroll_content.resize(n, Size::ZERO);
        self.text_shapes.clear();
        self.text_spans.clear();
        self.text_spans.resize(n, Span::default());
    }

    /// Summary of the arranged rects, for the cascade's
    /// incremental-validity check (`CascadeEngine::can_update`): it
    /// retains this value from its last full rebuild and compares to
    /// decide whether the rows it kept still describe the current
    /// arrangement.
    ///
    /// **Computed on demand, not cached on the struct.** Only the
    /// cascade wants it, and the cascade is skipped outright on most
    /// frames (`Ui::post_record`'s fingerprint gate). Refreshing it at
    /// the tail of every `LayoutEngine::run` would charge every layout
    /// pass — including the cache-hit ones that touch nothing else —
    /// for an answer usually nobody reads.
    ///
    /// Hashed as raw bytes rather than through `approx`'s visual
    /// quantisation on purpose: this gates a *cache-validity* decision,
    /// so it must be at least as strict as the exact element-wise
    /// comparison it replaces. Quantising would let a sub-quantum
    /// arrange shift retain a cascade built for the old rects. `Rect`
    /// is `Pod`, so the whole column hashes in one bulk write — the
    /// per-field form cost four hash rounds per rect instead of two.
    pub(crate) fn rect_hash(&self) -> ContentHash {
        let mut h = Hasher::new();
        h.write_usize(self.rect.len());
        h.pod_slice(&self.rect);
        ContentHash(h.finish())
    }
}
