//! A node's pre-order subtree bound, with the grid flag packed beside it.

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
    /// close. The debug assertion on the 31-bit ceiling catches a future
    /// overflow before it corrupts the flag.
    #[inline]
    pub(crate) fn new_open(id: u32, is_grid: bool) -> Self {
        debug_assert!(
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
