//! One node of the flat split tree, and the index that addresses it.

use serde::{Deserialize, Serialize};

use crate::primitives::layout::axis::Axis;
use crate::widgets::dock::tab_group::TabGroup;
use crate::widgets::splitter::split_half::SplitHalf;

/// Index of a node in [`DockState`](crate::DockState)'s flat tree; valid only between structural changes, unlike [`TabGroupId`](crate::TabGroupId). A stale one names another node, or none, where `DockState::node` panics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct NodeIdx(pub(super) u32);

impl NodeIdx {
    /// The index of slot `slot`.
    pub(super) fn at(slot: usize) -> Self {
        Self(
            u32::try_from(slot).expect("a dock tree's depth cap keeps it far below u32::MAX nodes"),
        )
    }

    pub(super) const fn usize(self) -> usize {
        self.0 as usize
    }
}

/// One node of the flat tree: a division, or a pane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum DockNode<T> {
    /// A division of this node's rect.
    Split(DockSplit),
    /// A leaf pane with one tab strip.
    Group(TabGroup<T>),
}

/// A division of one rect between two child nodes along `axis`: `first` leads, left along
/// [`Axis::X`] and top along [`Axis::Y`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DockSplit {
    pub(super) axis: Axis,
    pub(super) ratio: f32,
    pub(super) first: NodeIdx,
    pub(super) second: NodeIdx,
}

impl DockSplit {
    /// The axis the two children lie along.
    pub(crate) const fn axis(self) -> Axis {
        self.axis
    }

    /// The first child's share of the free space, inside the split clamp.
    pub(crate) const fn ratio(self) -> f32 {
        self.ratio
    }

    /// The child on `half`.
    pub(super) const fn child(self, half: SplitHalf) -> NodeIdx {
        match half {
            SplitHalf::First => self.first,
            SplitHalf::Second => self.second,
        }
    }
}
