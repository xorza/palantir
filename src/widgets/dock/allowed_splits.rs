//! Which splits a dock offers.

use crate::widgets::dock::split_side::{SplitDirection, SplitSide};

flag_set! {
    /// The split directions a [`DockView`](crate::DockView) offers during a tab drag; a refused
    /// one degrades to a join. `ALL` offers both, and `NONE` lets a dragged tab only join a strip.
    pub struct AllowedSplits: u8 {
        /// Side-by-side splits.
        const ROW = 1 << 0;
        /// Stacking splits.
        const COL = 1 << 1;
    }
}

impl AllowedSplits {
    /// Whether a split onto `side` is offered.
    pub(crate) const fn allows(self, side: SplitSide) -> bool {
        self.contains(match side.direction() {
            SplitDirection::Row => Self::ROW,
            SplitDirection::Col => Self::COL,
        })
    }
}
