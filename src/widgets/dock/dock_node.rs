//! One node of the flat split tree, and the index that addresses it.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::split_side::SplitDir;
use crate::widgets::dock::tab_group::TabGroup;

/// Index of a node in [`DockState`](crate::DockState)'s flat tree.
///
/// Only stable between structural changes — every one of them re-packs
/// the vector — so long-lived references use
/// [`TabGroupId`](crate::TabGroupId) instead, and an op fed a stale
/// index bounds-checks and no-ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeIdx(pub(crate) u32);

impl NodeIdx {
    pub(crate) fn usize(self) -> usize {
        self.0 as usize
    }
}

/// One node of the flat tree: a division, or a pane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DockNode<T> {
    Split(DockSplit),
    Group(TabGroup<T>),
}

/// A division of one rect between two child nodes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockSplit {
    pub dir: SplitDir,
    /// The first child's share of the free space.
    pub ratio: f32,
    pub first: NodeIdx,
    pub second: NodeIdx,
}
