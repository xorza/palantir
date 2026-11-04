//! Every piece of grid-layout scratch the engine holds, in one bag.

use crate::layout::depth_scratch::DepthScratch;
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;

/// All grid-layout scratch held by `LayoutEngine`: each grid's tracks, and the per-track
/// aggregator `Grid::intrinsic` extends by its tracks and truncates back on exit, which a nested
/// grid's query extends past.
#[derive(Debug, Default)]
pub(crate) struct GridCtx {
    pub(crate) track_state: GridTrackStore,
    pub(crate) track_aggregator: DepthScratch<f32>,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::layout::drivers::grid::grid_ctx::GridCtx;

    impl GridCtx {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                track_state,
                track_aggregator,
            } = self;
            track_state.scratch_bytes() + track_aggregator.scratch_bytes()
        }
    }
}
