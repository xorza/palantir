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
    pub(crate) const fn usize(self) -> usize {
        self.0 as usize
    }
}

/// One node of the flat tree: a division, or a pane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DockNode<T> {
    /// A division of this node's rect between two children.
    Split(DockSplit),
    /// A leaf pane, holding one tab strip.
    Group(TabGroup<T>),
}

/// A division of one rect between two child nodes.
///
/// Read-only outside the crate: a [`DockState`](crate::DockState) keeps its
/// ratio inside the split clamp, and only its ops change one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockSplit {
    pub(crate) dir: SplitDir,
    pub(crate) ratio: f32,
    pub(crate) first: NodeIdx,
    pub(crate) second: NodeIdx,
}

impl DockSplit {
    /// How the two children are arranged.
    pub const fn dir(self) -> SplitDir {
        self.dir
    }

    /// The first child's share of the free space, inside the split clamp.
    pub const fn ratio(self) -> f32 {
        self.ratio
    }

    /// The leading child — left in a [`SplitDir::Row`], top in a
    /// [`SplitDir::Column`].
    pub const fn first(self) -> NodeIdx {
        self.first
    }

    /// The trailing child.
    pub const fn second(self) -> NodeIdx {
        self.second
    }
}
