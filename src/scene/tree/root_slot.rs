//! One top-level subtree within a layer's tree, and where it is placed.

use crate::layout::types::placement::Placement;
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
