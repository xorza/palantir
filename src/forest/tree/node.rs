//! Per-NodeId record stored in `Tree`'s SoA arena.

use crate::forest::element::columns::{LayoutCore, NodeFlags};
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use soa_rs::Soars;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

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
    /// Layout-pass column: geometry + visibility. Bundled because the
    /// hot measure/arrange path reads all six fields together.
    pub layout: LayoutCore,
    /// Packed paint/input flags (2 B: sense / disabled / clip /
    /// focusable). Read by cascade / encoder / hit-test.
    pub attrs: NodeFlags,
}

const SUBTREE_GRID_FLAG: u32 = 1 << 31;
const SUBTREE_END_MASK: u32 = !SUBTREE_GRID_FLAG;

/// Exclusive pre-order subtree end with the "subtree (inclusive)
/// contains a `LayoutMode::Grid` node" flag packed into the high bit.
/// The low 31 bits hold the real end — arena will never approach
/// 2^31 nodes. Packed alongside the end (rather than a separate
/// `has_grid` bitset) so the `MeasureCache` grid-hug fast path tests
/// one load against the same SoA column the caller already touches for
/// the subtree bound.
///
/// Wrapping the raw word is load-bearing: [`Self::end`] and
/// [`Self::has_grid`] are the *only* reads and there is no raw-`u32`
/// accessor, so a new tree-walk can't forget the mask and silently read
/// `real + 2^31` for grid subtrees.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubtreeEnd(u32);

impl SubtreeEnd {
    /// A just-opened node: end is `id + 1` (covers only itself); the
    /// grid flag is set iff the node is itself a `LayoutMode::Grid`.
    /// Descendants fold their ends in via [`Self::merge_child`] at
    /// close. Release `assert!` on the 31-bit ceiling — a future
    /// overflow would corrupt the flag, so fail loudly.
    #[inline]
    pub(crate) fn new_open(id: u32, is_grid: bool) -> Self {
        assert!(
            id & SUBTREE_GRID_FLAG == 0,
            "NodeId {id} exhausted the 31-bit arena (high bit is the grid flag)",
        );
        let end = id + 1;
        Self(if is_grid {
            end | SUBTREE_GRID_FLAG
        } else {
            end
        })
    }

    /// Exclusive pre-order end, grid flag stripped.
    #[inline]
    pub(crate) fn end(self) -> u32 {
        self.0 & SUBTREE_END_MASK
    }

    /// `true` iff the subtree rooted here (inclusive) contains a
    /// `LayoutMode::Grid` node.
    #[inline]
    pub(crate) fn has_grid(self) -> bool {
        self.0 & SUBTREE_GRID_FLAG != 0
    }

    /// Fold a just-closed child into this (parent) end: take the larger
    /// pre-order end and union the grid flags. Bit-level: the low 31
    /// bits are always ≤ `SUBTREE_END_MASK` so `.max` on the masked
    /// words gives the right end; the high bit is the flag and
    /// `(a | b) & FLAG` unions cleanly.
    #[inline]
    pub(crate) fn merge_child(&mut self, child: SubtreeEnd) {
        self.0 = (self.0 & SUBTREE_END_MASK).max(child.0 & SUBTREE_END_MASK)
            | ((self.0 | child.0) & SUBTREE_GRID_FLAG);
    }
}
