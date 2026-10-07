//! Every piece of grid-layout scratch the engine holds, in one bag.

use crate::layout::drivers::grid::grid_depth_stack::GridDepthStack;
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;

/// All grid-layout scratch held by `LayoutEngine`. `depth_stack` and `track_state` are separate fields so callers can disjoint-borrow them; `track_aggregator` is a bump stack for `Grid::intrinsic`'s per-track aggregator, each call extending by `n_tracks` and truncating back to its base.
#[derive(Debug, Default)]
pub(crate) struct GridContext {
    pub(crate) depth_stack: GridDepthStack,
    pub(crate) track_state: GridTrackStore,
    pub(crate) track_aggregator: Vec<f32>,
}
