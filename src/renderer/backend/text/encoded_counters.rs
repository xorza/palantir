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
//! promotion signal at all: under `RenderKind::Partial` the encoder walk is
//! culled to the damage region, so a run that is on screen and unchanged is
//! not consulted on frames that do not damage it. [`EncodedCounters::hits`]
//! against [`EncodedCounters::encodes`] is what answers that, and
//! [`EncodedCounters::refiles`] says what the sweep pays per frame to keep
//! the population alive.
//!
//! Counters accumulate for the life of the backend, so readers take a
//! delta.

use crate::common::counters::counter_snapshot;

counter_snapshot! {
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
    encodes,
    /// Runs emitted from a resident template, origin-shifted.
    hits,
    /// Rows dropped by the sweep because their window lapsed.
    expiries,
    /// Tickets whose row was still live, so the sweep re-filed rather
    /// than dropped. The per-frame drain cost, and the number a
    /// probation tier is meant to cut.
    refiles,
    /// Rows that took a recycled block off a free list.
    ///
    /// Paired with [`Self::block_allocs`], this is the whole health
    /// check on the block allocator: recycling is what replaced the
    /// arena compaction, and it only holds if a gesture's rows keep
    /// landing in size classes their predecessors freed. `reuses`
    /// climbing while `allocs` stays flat is the statement "the arena
    /// has reached its working set and stopped growing".
    block_reuses,
    /// Rows that had to extend the arena because their size class had no
    /// free block. Expected during warm-up and whenever a genuinely new
    /// row length appears; a workload where this never settles is one
    /// whose row lengths keep drifting across class boundaries, and its
    /// arena grows to the sum of every class's peak.
    block_allocs,
}
