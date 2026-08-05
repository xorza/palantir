//! Observability for the shaped-buffer cache. Built on
//! [`TestOnly`](crate::common::counters::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! ## What this exists to separate
//!
//! Every entry point into [`CosmicMeasure`] returns a measurement
//! whether it reshaped or answered from cache, so from outside the two
//! are indistinguishable — and reshaping is the entire cost the cache
//! exists to avoid. [`TextShaper::measure_calls`] counts *dispatches* a
//! layer up and explicitly does not answer this; its own doc says
//! "cosmic may still hit its shaped-buffer cache, so the counter tracks
//! dispatches, not reshapes".
//!
//! Retention needs the same treatment. A buffer's lifetime turns on
//! three events — inserted, looked up, superseded — and which of them
//! fired is what separates a resize drag from a scroll, not how many
//! buffers happen to be resident afterwards.
//!
//! Counters accumulate for the life of the shaper, which outlives any
//! one frame and is shared across windows, so readers take a delta.
//!
//! [`CosmicMeasure`]: crate::text::cosmic::CosmicMeasure
//! [`TextShaper::measure_calls`]: crate::text::shaper::TextShaper

use crate::common::counters::TestOnly;

/// What the shaped-buffer cache did.
///
/// The counters are reached through directly rather than through
/// per-field forwarders: [`TestOnly`] already owns the gate, so
/// `probe.hits.bump()` is the whole call site and a wrapper would only
/// restate the field name.
#[derive(Debug, Default)]
pub(super) struct CacheCounters {
    /// Runs actually pushed through cosmic — `set_text` plus
    /// `shape_until_scroll`. The cost every other counter here exists to
    /// explain.
    pub(super) shapes: TestOnly<u32>,
    /// Lookups answered from the cache, layout-side and render-side
    /// alike.
    pub(super) hits: TestOnly<u32>,
    /// Entries demoted to the probation window because the reuse slot
    /// that owned them moved on to a different key.
    pub(super) supersedes: TestOnly<u32>,
    /// Buffers dropped by the end-of-frame sweep.
    pub(super) expiries: TestOnly<u32>,
    /// Times the "…" advance had to be reshaped because no slot held
    /// that face. Separate from `shapes`, which counts runs that landed
    /// in the cache — the ellipsis probe shapes without inserting.
    pub(super) ellipsis_misses: TestOnly<u32>,
}

/// Reads are test-only: nothing in a shipping build has a reason to ask,
/// and gating them here is what lets the counters themselves be absent.
#[cfg(test)]
impl CacheCounters {
    pub(super) fn counts(&self) -> CacheCounts {
        CacheCounts {
            shapes: self.shapes.count(),
            hits: self.hits.count(),
            supersedes: self.supersedes.count(),
            expiries: self.expiries.count(),
            ellipsis_misses: self.ellipsis_misses.count(),
        }
    }
}

/// One reading of a [`CacheCounters`]'s tallies. Subtract two to get what a
/// span of frames did — the counters accumulate for the life of the
/// shaper. Copied out rather than borrowed so a test can hold a "before"
/// reading across calls that need the shaper again.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct CacheCounts {
    pub(crate) shapes: u32,
    pub(crate) hits: u32,
    pub(crate) supersedes: u32,
    pub(crate) expiries: u32,
    pub(crate) ellipsis_misses: u32,
}

#[cfg(test)]
impl std::ops::Sub for CacheCounts {
    type Output = Self;

    fn sub(self, base: Self) -> Self {
        Self {
            shapes: self.shapes - base.shapes,
            hits: self.hits - base.hits,
            supersedes: self.supersedes - base.supersedes,
            expiries: self.expiries - base.expiries,
            ellipsis_misses: self.ellipsis_misses - base.ellipsis_misses,
        }
    }
}
