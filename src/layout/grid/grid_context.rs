//! Every piece of grid-layout scratch the engine holds, in one bag.

use crate::layout::grid::grid_depth_stack::GridDepthStack;
use crate::layout::grid::grid_track_store::GridTrackStore;

/// All grid-layout scratch held by `LayoutEngine`, in one bag. `depth_stack`
/// and `track_state` are separate fields so callers can disjoint-borrow them —
/// `AxisScratch::resolve_axis` takes `&mut self` (from `depth_stack`) and `&[f32]`
/// hug slices (from `track_state`) in the same expression via destructuring.
/// `track_aggregator` is a bump-stack scratch for `Grid::intrinsic`'s
/// per-track aggregator: each call extends by `n_tracks`, recurses (which
/// may extend further but always truncates back), then truncates to its
/// own base. Capacity retained.
#[derive(Debug, Default)]
pub(crate) struct GridContext {
    pub(crate) depth_stack: GridDepthStack,
    pub(crate) track_state: GridTrackStore,
    pub(crate) track_aggregator: Vec<f32>,
}
