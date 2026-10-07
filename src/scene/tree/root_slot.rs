//! One top-level subtree within a layer's tree, and where it is placed.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::placement::Placement;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One root within a layer's [`Tree`](crate::scene::tree::Tree); a popup records two (eater and body).
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: NodeId,
    pub(crate) placement: Placement,
}

impl RootSlot {
    /// Size offered to this root on `layer`.
    ///
    /// `LayoutEngine::run` and `MeasureCache::matches_forest` both read it and must agree, or every root misses.
    pub(crate) fn available(&self, layer: Layer, surface: Rect) -> Size {
        if layer == Layer::Main {
            surface.size
        } else {
            self.placement.available(surface)
        }
    }

    /// Where this root's slot of `size` starts on `layer`.
    pub(crate) fn origin(&self, layer: Layer, size: Size, surface: Rect) -> Vec2 {
        if layer == Layer::Main {
            surface.min
        } else {
            self.placement.origin(size, surface)
        }
    }
}
