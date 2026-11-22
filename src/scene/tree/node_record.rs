//! Per-`NodeId` record stored in `Tree`'s SoA arena.

use crate::common::span::Span;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::subtree_end::SubtreeEnd;
use palantir_derive::Soa;

/// Per-NodeId record, one push per `open_node` finalized by `close_node`; stored as `NodeRecords` so passes
/// reading one or two fields don't pull the rest into cache.
#[derive(Soa, Clone, Copy, Debug)]
#[soa(NodeRecords)]
pub(crate) struct NodeRecord {
    pub widget_id: WidgetId,
    /// Span into `Tree.shapes` covering every shape recorded in this node's open→close window, descendants included;
    /// a `Span` (not `start` plus the next node) so shapes pushed after the only child closes still count.
    pub shape_span: Span,
    /// Exclusive end in NodeId space (one past the last pre-order descendant), packed with the Grid flag. See [`SubtreeEnd`].
    pub subtree_end: SubtreeEnd,
    /// Immediate parent, or [`NodeId::NONE`] for a root; with `subtree_end` it answers both tree directions in one
    /// load. Recorded because `open_node` has it in hand, where later passes would rebuild an ancestor stack.
    pub parent: NodeId,
    /// Layout-pass column (geometry + visibility), bundled as the hot measure/arrange path reads it whole.
    pub layout: LayoutCore,
    /// Packed paint/input flags (4 B), read by cascade, encoder, hit-test and input routing.
    pub attrs: NodeFlags,
    /// Optional indices into the sparse `bounds_table` / `panel_table` / `chrome_table`; a field, so `NodeRecords`
    /// guarantees one row per node.
    pub extras: ExtrasIdx,
}
