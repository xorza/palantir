//! A node's pre-order subtree bound, with the grid flag packed beside it.

const SUBTREE_GRID_FLAG: u32 = 1 << 31;
const SUBTREE_END_MASK: u32 = !SUBTREE_GRID_FLAG;

/// Exclusive pre-order subtree end, with a "subtree (inclusive) contains a `LayoutMode::Grid` node" flag in the
/// high bit (the low 31 bits hold the end). Packed together so the `MeasureCache` grid-hug fast path tests one load;
/// the raw word is wrapped so no tree-walk can forget the mask and read `real + 2^31`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::NoUninit)]
pub(crate) struct SubtreeEnd(u32);

impl SubtreeEnd {
    /// A just-opened node: end is `id + 1`; the grid flag is set iff the node is itself a Grid. Children fold in via
    /// [`Self::merge_child`]; the debug assertion on the 31-bit ceiling catches overflow before it corrupts the flag.
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

    #[inline]
    pub(crate) const fn end(self) -> u32 {
        self.0 & SUBTREE_END_MASK
    }

    /// Whether the node at pre-order index `i` has children: its subtree ends right after it exactly when it has none.
    /// Shared by the cascade's skip cursor and [`Tree::has_children`](crate::scene::tree::Tree::has_children).
    #[inline]
    pub(crate) const fn has_children(self, i: usize) -> bool {
        self.end() as usize != i + 1
    }

    #[inline]
    pub(crate) const fn has_grid(self) -> bool {
        self.0 & SUBTREE_GRID_FLAG != 0
    }

    /// Folds a just-closed child into this (parent) end: the larger pre-order end, grid flags unioned.
    #[inline]
    pub(crate) const fn merge_child(&mut self, child: SubtreeEnd) {
        let (own, theirs) = (self.0 & SUBTREE_END_MASK, child.0 & SUBTREE_END_MASK);
        let end = if own > theirs { own } else { theirs };
        self.0 = end | ((self.0 | child.0) & SUBTREE_GRID_FLAG);
    }
}
