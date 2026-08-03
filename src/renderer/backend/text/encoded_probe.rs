//! Observability for the encoded-run cache. Built on
//! [`TestOnly`](crate::common::probe::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! On the narrow gate: these counters were added to size a probation
//! tier for gesture churn, which the measurement then argued *against*
//! building for now — so the only reader is the retention test in
//! `encode.rs`, and `BenchOnly` would be the speculative widening that
//! gate's doc warns off.
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
//! chosen rather than after. The open question is whether a stable run
//! is looked up often enough for "has it been asked for again" to work
//! as a promotion signal at all: under `RenderKind::Partial` the encoder
//! walk is culled to the damage region, so a run that is on screen and
//! unchanged is not consulted on frames that do not damage it.
//! [`Self::hits`] against [`Self::encodes`] is what answers that, and
//! [`Self::refiles`] says what the sweep pays per frame to keep the
//! population alive.
//!
//! Counters accumulate for the life of the backend, so readers take a
//! delta.

use crate::common::probe::TestOnly;

/// What the encoded-run cache did.
#[derive(Debug, Default)]
pub(super) struct EncodedProbe {
    /// Runs pushed through the full miss path — glyph extraction through
    /// the shaper lease, then an atlas touch or rasterization per glyph.
    /// The cost every other counter here exists to explain.
    pub(super) encodes: TestOnly<u32>,
    /// Runs emitted from a resident template, origin-shifted.
    pub(super) hits: TestOnly<u32>,
    /// Rows dropped by the sweep because their window lapsed.
    pub(super) expiries: TestOnly<u32>,
    /// Tickets whose row was still live, so the sweep re-filed rather
    /// than dropped. The per-frame drain cost, and the number a
    /// probation tier is meant to cut.
    pub(super) refiles: TestOnly<u32>,
    /// Rows that took a recycled block off a free list.
    ///
    /// Paired with [`Self::block_allocs`], this is the whole health
    /// check on the block allocator: recycling is what replaced the
    /// arena compaction, and it only holds if a gesture's rows keep
    /// landing in size classes their predecessors freed. `reuses`
    /// climbing while `allocs` stays flat is the statement "the arena
    /// has reached its working set and stopped growing".
    pub(super) block_reuses: TestOnly<u32>,
    /// Rows that had to extend the arena because their size class had no
    /// free block. Expected during warm-up and whenever a genuinely new
    /// row length appears; a workload where this never settles is one
    /// whose row lengths keep drifting across class boundaries, and its
    /// arena grows to the sum of every class's peak.
    pub(super) block_allocs: TestOnly<u32>,
}

/// Reads are gated with the counters themselves.
#[cfg(test)]
impl EncodedProbe {
    pub(super) fn counts(&self) -> EncodedCounts {
        EncodedCounts {
            encodes: self.encodes.count(),
            hits: self.hits.count(),
            expiries: self.expiries.count(),
            refiles: self.refiles.count(),
            block_reuses: self.block_reuses.count(),
            block_allocs: self.block_allocs.count(),
        }
    }
}

/// One reading of an [`EncodedProbe`]'s tallies. Subtract two to get
/// what a span of frames did — the counters accumulate for the life of
/// the backend. Copied out rather than borrowed so a caller can hold a
/// "before" reading across calls that need the backend again.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EncodedCounts {
    pub(crate) encodes: u32,
    pub(crate) hits: u32,
    pub(crate) expiries: u32,
    pub(crate) refiles: u32,
    pub(crate) block_reuses: u32,
    pub(crate) block_allocs: u32,
}

#[cfg(test)]
impl std::ops::Sub for EncodedCounts {
    type Output = Self;

    fn sub(self, base: Self) -> Self {
        Self {
            encodes: self.encodes - base.encodes,
            hits: self.hits - base.hits,
            expiries: self.expiries - base.expiries,
            refiles: self.refiles - base.refiles,
            block_reuses: self.block_reuses - base.block_reuses,
            block_allocs: self.block_allocs - base.block_allocs,
        }
    }
}
