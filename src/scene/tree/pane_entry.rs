//! A backdrop pane the recorder met.

use crate::scene::tree::node_id::NodeId;

/// What paints a pane: its owner's chrome, or one of the owner's shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PaneSource {
    Chrome,
    Shape,
}

/// A pane recorded this frame: its owner, its row in the owner's paint span, as
/// `OpenFrame::paint_rows` counted it, and what paints it. Recorded in paint order, so the
/// entries sort by `(shape_idx, source)`: a chrome comes before the shapes of its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaneEntry {
    pub(crate) node: NodeId,
    /// The pane's shape in `Tree::shapes.records`; for a chrome, the index the node's first shape
    /// takes.
    pub(crate) shape_idx: u32,
    pub(crate) source: PaneSource,
    pub(crate) row: u32,
}
