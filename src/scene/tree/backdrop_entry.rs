//! A backdrop the recorder met.

use crate::scene::tree::node_id::NodeId;

/// A backdrop recorded this frame: its owner, the shape that paints it and its row in the owner's
/// paint span, as `OpenFrame::paint_rows` counted it. Recorded in paint order, so the entries sort
/// by `shape_idx`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BackdropEntry {
    pub(crate) node: NodeId,
    /// The backdrop's shape in `Tree::shapes.records`.
    pub(crate) shape_idx: u32,
    pub(crate) row: u32,
}
