//! Which splits a dock offers.

use crate::primitives::layout::axis::Axis;
use crate::widgets::dock::split_side::SplitSide;

flag_set! {
    /// The splits a [`DockView`](crate::DockView) offers during a tab drag; a refused
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
        self.contains(match side.axis() {
            Axis::X => Self::ROW,
            Axis::Y => Self::COL,
        })
    }
}
