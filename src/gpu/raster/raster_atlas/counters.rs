//! Observability for a [`RasterAtlas`](super::RasterAtlas). Built on
//! [`BenchOnly`](crate::common::counters::BenchOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! On the wider cell gate rather than test-only because the question
//! these answer — does a real workload drive the atlas into the regime
//! where eviction bills at all — is only reachable from the `text_atlas`
//! benchmark. Reads are narrower still, gated with that benchmark alone:
//! the atlas's own tests assert on individual cells rather than on a
//! whole reading.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells BenchOnly, reads cfg(feature = "bench");

    /// What a raster atlas paid to keep itself packed.
    pub(crate) struct AtlasCounters;

    /// One reading of an [`AtlasCounters`].
    pub(crate) struct AtlasCounts;

    /// Entries whose rectangle was handed back to make room for another.
    evictions: u32,
    /// Side doublings. A one-way ratchet, so a test proving the atlas
    /// held its size needs to see this stay flat.
    grows: u32,
    /// Slots the clock hand walked past, summed over every call. Divided
    /// by [`Self::evictions`] this is the hand's average stride, which is
    /// the whole health check on the policy: a healthy thrash state stops
    /// on the first or second slot, and a number that climbs toward the
    /// slab length means the skip conditions are rejecting nearly
    /// everything.
    ///
    /// The eviction count alone says nothing about what victim selection
    /// cost, because `allocate` asks for a victim in a loop — this is the
    /// product that actually bills.
    evict_scans: u64,
    /// Entries refused because they exceed the side's growth ceiling.
    ///
    /// Distinct from a plain full atlas, and the distinction is the
    /// point: a full atlas recovers on its own once the frame's pressure
    /// clears, while this one never does — the same entry is refused on
    /// every frame it is drawn. A non-zero reading means content is
    /// asking for rasters the configured budget cannot hold, so the
    /// answer is a bigger `max_bytes` or a size ladder that clamps
    /// earlier, not anything the atlas can do at runtime.
    oversized: u32,
}
