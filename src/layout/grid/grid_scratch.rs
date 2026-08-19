//! One nesting depth's worth of per-axis grid scratch.

use crate::layout::grid::axis_scratch::AxisScratch;

/// One grid's two axes of per-frame scratch. Capacity is retained
/// across frames;
/// [`GridDepthStack`](crate::layout::grid::grid_depth_stack::GridDepthStack)
/// owns the per-depth pool these come from.
#[derive(Debug, Default)]
pub(super) struct GridScratch {
    pub(super) col: AxisScratch,
    pub(super) row: AxisScratch,
}
