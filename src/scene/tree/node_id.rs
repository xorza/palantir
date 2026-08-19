//! The index of one node in a tree's SoA arena.

/// Index of one node in a [`Tree`](crate::scene::tree::Tree)'s SoA
/// arena, in record (pre-order) order. Every per-node column is indexed
/// by it, so it is the crate's node identity for every pass after
/// recording — distinct from [`WidgetId`](crate::WidgetId), which is the
/// author's identity and survives across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}
