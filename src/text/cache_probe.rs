//! Observability for the shaped-buffer cache. Built on
//! [`TestOnly`](crate::common::probe::TestOnly), whose module doc
//! explains the gated-cell pattern and why the two gates exist.
//!
//! On the narrow gate: nothing benches these yet, and the crate widens a
//! counter for a real bench rather than a hypothetical one.
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
//! [`TextShaper::measure_calls`]: crate::text::TextShaper

use crate::common::probe::TestOnly;

/// What the shaped-buffer cache did.
#[derive(Debug, Default)]
pub(super) struct CacheProbe {
    /// Runs actually pushed through cosmic — `set_text` plus
    /// `shape_until_scroll`. The cost every other counter here exists to
    /// explain.
    shapes: TestOnly<u32>,
    /// Lookups answered from the cache, layout-side and render-side
    /// alike.
    hits: TestOnly<u32>,
    /// Entries demoted to the probation window because the reuse slot
    /// that owned them moved on to a different key.
    supersedes: TestOnly<u32>,
    /// Buffers dropped by the end-of-frame sweep.
    expiries: TestOnly<u32>,
}

impl CacheProbe {
    #[inline]
    pub(super) fn shape(&mut self) {
        self.shapes.bump();
    }

    #[inline]
    pub(super) fn hit(&mut self) {
        self.hits.bump();
    }

    #[inline]
    pub(super) fn supersede(&mut self) {
        self.supersedes.bump();
    }

    #[inline]
    pub(super) fn expire(&mut self) {
        self.expiries.bump();
    }
}

/// Reads are test-only: nothing in a shipping build has a reason to ask,
/// and gating them here is what lets the counters themselves be absent.
#[cfg(test)]
impl CacheProbe {
    pub(super) fn shapes(&self) -> u32 {
        self.shapes.count()
    }

    pub(super) fn hits(&self) -> u32 {
        self.hits.count()
    }

    pub(super) fn supersedes(&self) -> u32 {
        self.supersedes.count()
    }

    pub(super) fn expiries(&self) -> u32 {
        self.expiries.count()
    }
}
