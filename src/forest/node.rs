//! Per-NodeId record stored in `Tree`'s SoA arena.

use crate::forest::element::{LayoutCore, NodeFlags};
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use soa_rs::Soars;

/// Per-NodeId record. One push per `open_node`, finalized by
/// `close_node`. Stored as `Soa<NodeRecord>` on `Tree.records` so
/// each field becomes its own contiguous slice — passes that read
/// only one or two fields don't pull the rest into cache.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct NodeRecord {
    /// Author-supplied identity. Read by hit-test, state map, damage diff.
    pub widget_id: WidgetId,
    /// Span into `Tree.shapes`: covers every shape recorded inside
    /// this node's open→close window, including descendants. `len` is
    /// set at `close_node` from `shapes.len() - start`. Stored as a
    /// `Span` (rather than just `start` + a "look at next node"
    /// trick) so a node with shapes pushed AFTER its only child closes
    /// — e.g. `Scroll` with bars at slot N — gets a correct count for
    /// the child's subtree.
    pub shape_span: Span,
    /// Exclusive end in NodeId space: one past the last descendant
    /// in pre-order. `i + 1 == subtree_end` for a leaf.
    pub subtree_end: u32,
    /// Layout-pass column: geometry + visibility. Bundled because the
    /// hot measure/arrange path reads all six fields together.
    pub layout: LayoutCore,
    /// 1-byte packed paint/input flags (sense / disabled / clip /
    /// focusable). Read by cascade / encoder / hit-test.
    pub attrs: NodeFlags,
}
