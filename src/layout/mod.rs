//! Layout: the measure and arrange passes over a recorded forest, and the per-plane rect tables they
//! produce. [`LayoutEngine`](engine::LayoutEngine) owns the scratch, text system and measure cache; the
//! finalized [`Layout`] is threaded out to cascade, encoder, hit testing and scroll refresh. Each
//! container kind is a [`LayoutDriver`](drivers::LayoutDriver) reached through one dispatch.

mod axis_align_pair;
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
pub(crate) mod layout_scratch;
pub(crate) mod measured;
pub(crate) mod pass;
mod placed_child;
pub(crate) mod plane_input;
pub(crate) mod plane_layout;
pub(crate) mod text;

use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::scene::endpoint::Endpoint;
use crate::scene::forest::Forest;
use crate::scene::per_plane::PerPlane;
use crate::scene::plane::Plane;
use std::ops::{Index, IndexMut};

/// Per-frame layout output across all planes, indexed by `Plane` (see [`PerPlane`]); filled in place.
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Private: `Index<Plane>` is the one way to a plane's columns.
    planes: PerPlane<PlaneLayout>,
}

impl Layout {
    /// Grow to a slot for every plane `forest` records into.
    pub(crate) fn cover(&mut self, forest: &Forest) {
        self.planes.cover(&forest.trees);
    }

    /// Measured content extent of the scroll viewport at `endpoint`, `ZERO` for non-`LayoutMode::Scroll` nodes.
    ///
    /// Takes an [`Endpoint`] ([`Cascade::endpoint`](crate::cascade::Cascade::endpoint)) to bridge the id-keyed and node-keyed tables.
    #[inline]
    pub(crate) fn scroll_content(&self, endpoint: Endpoint) -> Size {
        self.planes[endpoint.plane].scroll_content[endpoint.node.idx()]
    }

    /// The node's arranged rect: pre-transform, unclipped, world coords; the single home of arranged rects.
    #[inline]
    pub(crate) fn arranged_rect(&self, endpoint: Endpoint) -> Rect {
        self.planes[endpoint.plane].rect[endpoint.node.idx()]
    }
}

impl Index<Plane> for Layout {
    type Output = PlaneLayout;
    #[inline]
    fn index(&self, plane: Plane) -> &PlaneLayout {
        &self.planes[plane]
    }
}

impl IndexMut<Plane> for Layout {
    #[inline]
    fn index_mut(&mut self, plane: Plane) -> &mut PlaneLayout {
        &mut self.planes[plane]
    }
}

#[cfg(test)]
mod tests;
