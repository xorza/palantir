//! Which edge of a pane a new one lands on, and the split it implies.

use serde::{Deserialize, Serialize};

/// How a split arranges its children: `Row` side by side (vertical
/// divider), `Column` stacked (horizontal divider).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Children side by side, divided by a vertical rule.
    Row,
    /// Children stacked, divided by a horizontal rule.
    Column,
}

/// Which edge of a pane a split lands on — the new pane takes that
/// edge's half. `Left` / `Right` split into a [`SplitDirection::Row`],
/// `Top` / `Bottom` into a [`SplitDirection::Column`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitSide {
    /// The new pane takes the left half.
    Left,
    /// The new pane takes the right half.
    Right,
    /// The new pane takes the top half.
    Top,
    /// The new pane takes the bottom half.
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

    /// Whether the new pane becomes the split's *first* child (left or
    /// top).
    pub(crate) const fn new_pane_first(self) -> bool {
        matches!(self, SplitSide::Left | SplitSide::Top)
    }
}
