//! The index of one node in a tree's SoA arena.

/// Index of one node in a [`Tree`](crate::scene::tree::Tree)'s SoA arena, in record (pre-order) order: the crate's node identity after recording, distinct from the author's [`WidgetId`](crate::WidgetId), which survives across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    /// "No node" in the `parent` column, where a root has none; out of arena range since `SubtreeEnd` caps the arena at 31 bits.
    pub(super) const NONE: Self = Self(u32::MAX);

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self.0 as usize
    }
}
