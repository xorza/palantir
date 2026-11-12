//! Observability for a [`RasterAtlas`](super::RasterAtlas), on [`BenchOnly`](crate::common::counters::BenchOnly) cells:
//! whether a real workload drives eviction is only reachable from the `text_atlas` benchmark, which alone gates the reads.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(feature = "internals");

    pub(crate) struct AtlasCounters;

    pub(crate) struct AtlasCounts;

    /// Rasters the pool evicted, either tenant's insert taking their room, counted as the tenant
    /// learns of each: at a lookup that finds it gone, or at the drain.
    evictions: u32,
    /// Entries refused because they exceed a page: unlike a full atlas, they are refused every
    /// frame they are drawn, so content asks for rasters no page can hold.
    oversized: u32,
}
