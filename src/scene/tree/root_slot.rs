//! One top-level subtree within a layer's tree, and where it is placed.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::placement::Placement;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One root within a single layer's [`Tree`](crate::scene::tree::Tree).
/// Multiple roots in the same tree happen for popups (eater + body
/// recorded as two top-level scopes) and any future `Ui::layer` scope
/// that opens non-contiguous top-level subtrees in the same layer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: NodeId,
    pub(crate) placement: Placement,
}

impl RootSlot {
    /// Size offered to this root on `layer`.
    ///
    /// `Layer::Main` fills the surface; every overlay layer derives its
    /// own from [`Self::placement`]. Shared by `LayoutEngine::run`, which
    /// measures against it, and `MeasureCache::matches_forest`, which
    /// quantizes it into the snapshot's root key — those two **must**
    /// agree, or the key describes an offer measure never saw and every
    /// root misses.
    pub(crate) fn available(&self, layer: Layer, surface: Rect) -> Size {
        if layer == Layer::Main {
            surface.size
        } else {
            self.placement.available(surface)
        }
    }

    /// Where this root's slot of `size` starts on `layer`.
    ///
    /// `Layer::Main` starts at the surface's origin; every overlay layer
    /// resolves [`Self::placement`] against `size`. The arrange and the
    /// check that keeps the last run both read it here, so the two
    /// cannot disagree on where a root lands.
    pub(crate) fn origin(&self, layer: Layer, size: Size, surface: Rect) -> Vec2 {
        if layer == Layer::Main {
            surface.min
        } else {
            self.placement.origin(size, surface)
        }
    }
}
