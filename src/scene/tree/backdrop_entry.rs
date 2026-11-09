//! A backdrop the recorder met.

use crate::scene::tree::node_id::NodeId;

/// What paints a backdrop: its owner's chrome, or one of the owner's shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BackdropOrigin {
    Chrome,
    Shape,
}

/// A backdrop recorded this frame: its owner, its row in the owner's paint span, as
/// `OpenFrame::paint_rows` counted it, and what paints it. Recorded in paint order, so the
/// entries sort by `(shape_idx, origin)`: a chrome comes before the shapes of its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BackdropEntry {
    pub(crate) node: NodeId,
    /// The backdrop's shape in `Tree::shapes.records`; for a chrome, the index the node's first
    /// shape takes.
    pub(crate) shape_idx: u32,
    pub(crate) origin: BackdropOrigin,
    pub(crate) row: u32,
}
