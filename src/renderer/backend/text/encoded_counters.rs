//! Observability for the encoded-run cache. Built on
//! [`TestOnly`](crate::common::counters::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! These were added to size a probation tier for gesture churn, which
//! the measurement then argued *against* building for now — so every
//! reader is a test in `encode/tests.rs`.
//!
//! ## What this exists to separate
//!
//! From outside, a frame that replayed every run from a cached template
//! and one that re-encoded them all look identical: both emit the same
//! instances. The difference is the entire point of the cache — a miss
//! walks cosmic's layout runs and touches the atlas per glyph, a hit is
//! a memcpy with an origin shift.
//!
//! Retention needs the same treatment, and needs it *before* a policy is
//! chosen rather than after. The open question is whether a stable run is
//! looked up often enough for "has it been asked for again" to work as a
//! promotion signal at all: under `Damage::Partial` the encoder walk is
//! culled to the damage region, so a run that is on screen and unchanged is
//! not consulted on frames that do not damage it. [`EncodedCounters::hits`]
//! against [`EncodedCounters::encodes`] is what answers that, and
//! [`EncodedCounters::refiles`] says what the sweep pays per frame to keep
//! the population alive.
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
    /// The cost every other counter here exists to explain.
    encodes: u32,
    /// Runs emitted from a resident template, origin-shifted.
    hits: u32,
    /// Rows dropped by the sweep because their window lapsed.
    expiries: u32,
    /// Tickets whose row was still live, so the sweep re-filed rather
    /// than dropped. The per-frame drain cost, and the number a
    /// probation tier is meant to cut.
    refiles: u32,
}
