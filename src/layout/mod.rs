//! Layout: the measure and arrange passes over a recorded forest, and the per-layer rect tables they
//! produce. [`LayoutEngine`](engine::LayoutEngine) owns the scratch, text system and measure cache; the
//! finalized [`Layout`] is threaded out to cascade, encoder, hit testing and scroll refresh. Each
//! container kind is a [`LayoutDriver`](drivers::LayoutDriver) reached through one dispatch.

mod axis_align_pair;
mod axis_placement;
mod axis_share;
mod axis_slot;
pub(crate) mod cache;
pub(crate) mod counters;
pub(crate) mod depth_scratch;
pub(crate) mod drivers;
pub(crate) mod engine;
mod fill_item;
mod hug_item;
pub(crate) mod intrinsic;
mod justify_offsets;
pub(crate) mod layer_layout;
pub(crate) mod layout_scratch;
pub(crate) mod measured;
pub(crate) mod pass;
pub(crate) mod text;

use crate::layout::layer_layout::LayerLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::scene::endpoint::Endpoint;
use crate::scene::layer::Layer;
use crate::scene::per_layer::PerLayer;
use std::ops::{Index, IndexMut};

/// Per-frame layout output across all layers, indexed by `Layer` (see [`PerLayer`]); filled in place.
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Private: `Index<Layer>` is the one way to a layer's columns.
    layers: PerLayer<LayerLayout>,
}

impl Layout {
    /// Measured content extent of the scroll viewport at `endpoint`, `ZERO` for non-`LayoutMode::Scroll` nodes.
    ///
    /// Takes an [`Endpoint`] ([`Cascade::endpoint`](crate::cascade::Cascade::endpoint)) to bridge the id-keyed and node-keyed tables.
    #[inline]
    pub(crate) fn scroll_content(&self, endpoint: Endpoint) -> Size {
        self.layers[endpoint.layer].scroll_content[endpoint.node.idx()]
    }

    /// The node's arranged rect: pre-transform, unclipped, world coords; the single home of arranged rects.
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

#[cfg(test)]
mod tests;
