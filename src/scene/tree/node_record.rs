//! Per-`NodeId` record stored in `Tree`'s SoA arena.

use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::subtree_end::SubtreeEnd;
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
    /// in pre-order, packed with the "subtree contains a Grid" flag.
    /// `i + 1 == end()` for a leaf. See [`SubtreeEnd`].
    pub subtree_end: SubtreeEnd,
    /// Immediate parent, or [`NodeId::NONE`] for a root. The
    /// complement of `subtree_end`: together they answer both
    /// directions of the tree in one indexed load.
    ///
    /// Recorded rather than re-derived. `open_node` has the parent in
    /// hand, where every later pass would have to rebuild an ancestor
    /// stack keyed on `subtree_end` to get it back — which the damage
    /// diff did, once per node of the outer walk and again per node of
    /// every moved subtree.
    pub parent: NodeId,
    /// Layout-pass column: geometry + visibility. Bundled because the
    /// hot measure/arrange path reads all six fields together.
    pub layout: LayoutCore,
    /// Packed paint/input flags (2 B: sense / disabled / clip /
    /// focusable). Read by cascade / encoder / hit-test.
    pub attrs: NodeFlags,
    /// Optional two-byte indices into the sparse `bounds_table` /
    /// `panel_table` / `chrome_table`. A field rather than a `Vec`
    /// beside `records`, so "one row per node" is what `Soa` already
    /// guarantees instead of a `debug_assert` on two lengths — a
    /// missed push cannot happen when there is only one push. Rides
    /// free: 6 B at align 2 fills the 8 B slot `attrs` opens.
    pub extras: ExtrasIdx,
}
