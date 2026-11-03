//! One top-level subtree within a plane's tree, and where it is placed.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::placement::Placement;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// One root within a plane's [`Tree`](crate::scene::tree::Tree); a popup records two (barrier and body).
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: NodeId,
    pub(crate) placement: Placement,
    /// The layer the root asked for, which can sit below its plane's (see `Forest::push_layer`);
    /// `Main` for ordinary content.
    pub(crate) layer: Layer,
}

impl RootSlot {
    /// Size offered to this root.
    ///
    /// `LayoutEngine::run` measures at it and keys the root's run by it, so the two agree.
    pub(crate) fn available(&self, surface: Rect) -> Size {
        if self.layer == Layer::Main {
            surface.size
        } else {
            self.placement.available(surface)
        }
    }

    /// Where this root's slot of `size` starts.
    pub(crate) fn origin(&self, size: Size, surface: Rect) -> Vec2 {
        if self.layer == Layer::Main {
            surface.min
        } else {
            self.placement.origin(size, surface)
        }
    }
}
