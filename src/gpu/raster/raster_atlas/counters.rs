//! Observability for a [`RasterAtlas`](super::RasterAtlas), on [`BenchOnly`](crate::common::counters::BenchOnly) cells:
//! whether a real workload drives eviction is only reachable from the `text_atlas` benchmark, which alone gates the reads.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(feature = "internals");

    pub(crate) struct AtlasCounters;

    pub(crate) struct AtlasCounts;

    evictions: u32,
    /// The most entries one insert evicted. Reclaim is per rectangle, so this stays near one; a
    /// rise means victims sat on shelves the inserts could not use.
    worst_insert_evictions: u32,
    /// Side doublings. A one-way ratchet, so a test proving the atlas held its size needs this flat.
    grows: u32,
    /// Slots the clock hand walked past, summed over every call. Over [`Self::evictions`] it is the average stride, the
    /// policy's health check: healthy thrash stops on the first or second slot.
    evict_scans: u64,
    /// Entries refused because they exceed the side's growth ceiling: unlike a full atlas, they are refused every frame
    /// they are drawn, so content asks for rasters the budget cannot hold.
    oversized: u32,
}
