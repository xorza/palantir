//! Observability for the cascade pass. Built on [`TestOnly`], whose module
//! doc explains the gated-cell pattern and why the two gates exist.
//!
//! This pass is the sharpest case for the accumulate default:
//! `FrameCycle::post_record`'s fingerprint gate skips
//! [`CascadeEngine::run`] outright on an unchanged frame, so a per-pass
//! reset would not fire at all on those frames and each of them would
//! report the previous run's numbers as its own.
//!
//! [`CascadeEngine::run`]: crate::scene::cascade::engine::CascadeEngine

use crate::common::counters::TestOnly;

/// What the cascade did, for tests to assert against.
///
/// The two counters exist as a pair: both paths end in the same correct
/// cascade, so from the outside there is nothing to tell them apart, and
/// separating "`can_update` said no" from "the incremental walk gave up
/// halfway" is the whole point.
#[derive(Debug, Default)]
pub(crate) struct CascadeCounters {
    /// Full rebuilds performed.
    full_rebuilds: TestOnly<u32>,
    /// Incremental walks that got partway and gave up, forcing the full
    /// rebuild they had already started duplicating.
    abandoned_incrementals: TestOnly<u32>,
}

impl CascadeCounters {
    #[inline]
    pub(crate) fn full_rebuild(&mut self) {
        self.full_rebuilds.bump();
    }

    #[inline]
    pub(crate) fn abandoned_incremental(&mut self) {
        self.abandoned_incrementals.bump();
    }
}

/// Reads are test-only: nothing in a shipping build has a reason to ask,
/// and gating them here is what lets the counters themselves be absent.
#[cfg(test)]
impl CascadeCounters {
    pub(crate) fn full_rebuilds(&self) -> u32 {
        self.full_rebuilds.count()
    }

    pub(crate) fn abandoned_incrementals(&self) -> u32 {
        self.abandoned_incrementals.count()
    }
}
