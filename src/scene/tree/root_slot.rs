//! One top-level subtree within a layer's tree, and where it is placed.

use crate::layout::types::placement::Placement;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;

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
}
