//! Observability for the encoded-run cache. Built on
//! [`TestOnly`](crate::common::counters::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! ## What this exists to separate
//!
//! From outside, a frame that replayed every run from a cached template
//! and one that re-encoded them all look identical: both emit the same
//! instances. The difference is the entire point of the cache — a miss
//! walks cosmic's layout runs and touches the atlas per glyph, a hit is
//! a memcpy with an origin shift. [`EncodedCounters::encodes`] is what
//! separates them.
//!
//! Retention is the other half. A gesture mints a key a frame, so the
//! population is bounded by the window rather than by what is on screen,
//! and [`EncodedCounters::expiries`] against the rows still resident is
//! what shows that bound holding. [`EncodedCounters::refiles`] is what
//! the sweep pays per frame to keep the live rows alive.
//!
//! What the *arena* under the cache did — blocks allocated against
//! blocks recycled — is
//! [`BlockArenaCounters`](crate::common::block_arena::BlockArenaCounters)
//! instead, read off `EncodedCache::arena`. It belongs to the allocator
//! rather than to this cache because the paint-snapshot arena asks the
//! same question of the same mechanism.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    /// What the encoded-run cache did.
    pub(super) struct EncodedCounters;

    /// One reading of an [`EncodedCounters`]'s tallies. Subtract two to get
    /// what a span of frames did — the counters accumulate for the life of
    /// the backend. Copied out rather than borrowed so a caller can hold a
    /// "before" reading across calls that need the backend again.
    pub(crate) struct EncodedCounts;

    /// Runs pushed through the full miss path — glyph extraction through
    /// the shaper lease, then an atlas touch or rasterization per glyph.
    /// Zero on a frame that replayed every run from its template.
    encodes: u32,
    /// Rows dropped by the sweep because their window lapsed.
    expiries: u32,
    /// Tickets whose row was still live, so the sweep re-filed rather
    /// than dropped — what the per-frame drain costs above the rows it
    /// actually retires.
    refiles: u32,
}
