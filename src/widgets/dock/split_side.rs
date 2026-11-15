//! Which edge of a pane a new one lands on, and the split it implies.

use crate::primitives::layout::axis::Axis;
use crate::widgets::splitter::split_half::SplitHalf;
use serde::{Deserialize, Serialize};

/// Which edge of a pane a split lands on; `Left`/`Right` split along [`Axis::X`], `Top`/`Bottom`
/// along [`Axis::Y`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The axis the split this side implies lays its panes along.
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::X,
            Self::Top | Self::Bottom => Axis::Y,
        }
    }

    /// The half the new pane takes: the first for left or top.
    pub const fn half(self) -> SplitHalf {
        match self {
            Self::Left | Self::Top => SplitHalf::First,
            Self::Right | Self::Bottom => SplitHalf::Second,
        }
    }
}
