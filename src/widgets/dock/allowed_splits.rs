//! Which splits a dock offers.

use crate::widgets::dock::split_side::{SplitDir, SplitSide};

/// The split directions a [`DockView`](crate::DockView) offers while a
/// tab is dragged. A refused direction degrades to a join, so the widget
/// never shows a drop the model would go on to refuse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllowedSplits {
    #[default]
    All,
    /// Only splits that put the two panes side by side.
    Row,
    /// Only splits that stack the two panes.
    Column,
    /// No splits — a dragged tab can only join another strip.
    None,
}

impl AllowedSplits {
    /// Whether a split onto `side` is offered.
    pub fn allows(self, side: SplitSide) -> bool {
        match self {
            Self::All => true,
            Self::Row => side.dir() == SplitDir::Row,
            Self::Column => side.dir() == SplitDir::Column,
            Self::None => false,
        }
    }
}
