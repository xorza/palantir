//! One node of the flat split tree, and the index that addresses it.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::split_side::SplitDirection;
use crate::widgets::dock::tab_group::TabGroup;

/// Index of a node in [`DockState`](crate::DockState)'s flat tree; valid only between structural changes (a stale one no-ops), unlike [`TabGroupId`](crate::TabGroupId).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeIndex(pub(crate) u32);

impl NodeIndex {
    pub(crate) const fn usize(self) -> usize {
        self.0 as usize
    }
}

/// One node of the flat tree: a division, or a pane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DockNode<T> {
    /// A division of this node's rect.
    Split(DockSplit),
    /// A leaf pane with one tab strip.
    Group(TabGroup<T>),
}

/// A division of one rect between two child nodes; read-only outside the crate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockSplit {
    pub(crate) direction: SplitDirection,
    pub(crate) ratio: f32,
    pub(crate) first: NodeIndex,
    pub(crate) second: NodeIndex,
}

impl DockSplit {
    /// How the two children are arranged.
    pub const fn direction(self) -> SplitDirection {
        self.direction
    }

    /// The first child's share of the free space, inside the split clamp.
    pub const fn ratio(self) -> f32 {
        self.ratio
    }

    /// The leading child: left in a Row, top in a Column.
    pub const fn first(self) -> NodeIndex {
        self.first
    }

    /// The trailing child.
    pub const fn second(self) -> NodeIndex {
        self.second
    }
}
