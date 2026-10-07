//! Observability for the encoded-run cache, on [`TestOnly`](crate::common::counters::TestOnly) cells.
//!
//! [`EncodedCounters::encodes`] tells a frame that replayed every run from a cached template from one that
//! re-encoded them (both emit the same instances); [`EncodedCounters::expiries`] against resident rows shows
//! the retention bound holding. The arena under the cache is counted by
//! [`BlockArenaCounters`](crate::common::block_arena::BlockArenaCounters) via `EncodedCache::arena`.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    pub(super) struct EncodedCounters;

    /// One reading of an [`EncodedCounters`]'s tallies; subtract two to get what a span of frames did.
    pub(crate) struct EncodedCounts;

    /// Runs through the full miss path (glyph extraction, atlas touch or raster per glyph); zero on full replay.
    encodes: u32,
    /// Rows dropped by the sweep because their window lapsed.
    expiries: u32,
    /// Tickets whose row was still live, so the sweep re-filed rather than dropped.
    refiles: u32,
}
