//! One nesting depth's worth of per-axis grid scratch.

use crate::layout::drivers::grid::axis_scratch::AxisScratch;

/// One grid's two axes of per-frame scratch, capacity retained; the per-depth pool is [`GridDepthStack`](crate::layout::drivers::grid::grid_depth_stack::GridDepthStack).
#[derive(Debug, Default)]
pub(super) struct GridScratch {
    pub(super) col: AxisScratch,
    pub(super) row: AxisScratch,
}
