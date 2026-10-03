//! Layout: the measure and arrange passes over a recorded forest, and the
//! per-layer rect tables they produce.
//!
//! [`LayoutEngine`](engine::LayoutEngine) owns the scratch, the text system
//! and the cross-frame measure cache. The finalized [`Layout`] is threaded
//! out to the caller instead of held here, because the cascade, the encoder,
//! hit testing and scroll refresh all read it.
//!
//! Each container kind is a [`LayoutDriver`](driver::LayoutDriver), and the
//! three passes reach one through the single dispatch in that module.

pub(crate) mod axis;
mod axis_align_pair;
mod axis_placement;
mod axis_share;
mod axis_slot;
pub(crate) mod cache;
mod canvas;
pub(crate) mod counters;
pub(crate) mod depth_scratch;
mod driver;
pub(crate) mod engine;
mod fill_item;
pub(crate) mod grid;
mod hug_item;
pub(crate) mod intrinsic;
mod justify_offsets;
pub(crate) mod layer_layout;
pub(crate) mod layout_scratch;
pub(crate) mod measured;
pub(crate) mod pass;
pub(crate) mod scroll;
pub(crate) mod scrollbars;
pub(crate) mod shaped_text;
pub(crate) mod stack;
pub(crate) mod text_runs;
pub(crate) mod text_shape_input;
pub(crate) mod types;
pub(crate) mod wrapstack;
pub(crate) mod zstack;

#[cfg(test)]
mod cross_driver_tests;

use crate::layout::layer_layout::LayerLayout;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::per_layer::PerLayer;
use std::ops::{Index, IndexMut};

/// Per-frame layout output across all layers. Callers index by
/// `Layer` directly (`result[Layer::Main]`) — see [`PerLayer`].
/// Filled in place by `LayoutEngine::run`, which takes it as `&mut` so the
/// buffers survive across frames; the encoder, cascade, hit-index,
/// and tests all read it afterwards. (The cascade pass's own output lives on
/// `Ui::cascade` — this struct is purely the layout pass's product.)
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Private: `Index<Layer>` is the one way to a layer's columns, so
    /// there is a single spelling to grep for. The two [`Endpoint`]
    /// accessors below sit alongside it because they answer a different
    /// question — they bridge a `WidgetId`-keyed caller into this
    /// `(layer, node)`-keyed table.
    layers: PerLayer<LayerLayout>,
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
