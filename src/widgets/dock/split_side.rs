//! Which edge of a pane a new one lands on, and the split it implies.

use serde::{Deserialize, Serialize};

/// How a split arranges its children: `Row` side by side (vertical
/// divider), `Column` stacked (horizontal divider).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDir {
    Row,
    Column,
}

/// Which edge of a pane a split lands on — the new pane takes that
/// edge's half. `Left` / `Right` split into a [`SplitDir::Row`],
/// `Top` / `Bottom` into a [`SplitDir::Column`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl SplitSide {
    pub fn dir(self) -> SplitDir {
        match self {
            SplitSide::Left | SplitSide::Right => SplitDir::Row,
            SplitSide::Top | SplitSide::Bottom => SplitDir::Column,
        }
    }

    /// Whether the new pane becomes the split's *first* child (left or
    /// top).
    pub(crate) fn new_pane_first(self) -> bool {
        matches!(self, SplitSide::Left | SplitSide::Top)
    }
}
