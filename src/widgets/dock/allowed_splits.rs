//! Which splits a dock offers.

use crate::widgets::dock::split_side::{SplitDirection, SplitSide};

/// The split directions a [`DockView`](crate::DockView) offers during a tab drag; a refused one degrades to a join.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllowedSplits {
    /// Every direction. The default.
    #[default]
    All,
    /// Only side-by-side splits.
    Row,
    /// Only stacking splits.
    Column,
    /// No splits; a dragged tab can only join a strip.
    None,
}

impl AllowedSplits {
    /// Whether a split onto `side` is offered.
    pub fn allows(self, side: SplitSide) -> bool {
        match self {
            Self::All => true,
            Self::Row => side.direction() == SplitDirection::Row,
            Self::Column => side.direction() == SplitDirection::Column,
            Self::None => false,
        }
    }
}
