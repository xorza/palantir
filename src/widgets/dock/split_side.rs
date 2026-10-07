//! Which edge of a pane a new one lands on, and the split it implies.

use serde::{Deserialize, Serialize};

/// How a split arranges its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Side by side, divided by a vertical rule.
    Row,
    /// Stacked, divided by a horizontal rule.
    Column,
}

/// Which edge of a pane a split lands on; `Left`/`Right` make a [`SplitDirection::Row`], `Top`/`Bottom` a [`SplitDirection::Column`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitSide {
    /// Left half.
    Left,
    /// Right half.
    Right,
    /// Top half.
    Top,
    /// Bottom half.
    Bottom,
}

impl SplitSide {
    /// The split this side implies.
    pub const fn direction(self) -> SplitDirection {
        match self {
            SplitSide::Left | SplitSide::Right => SplitDirection::Row,
            SplitSide::Top | SplitSide::Bottom => SplitDirection::Column,
        }
    }

    /// Whether the new pane is the split's first child (left or top).
    pub(crate) const fn new_pane_first(self) -> bool {
        matches!(self, SplitSide::Left | SplitSide::Top)
    }
}
