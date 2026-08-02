//! Test-only observability for the layout pass.
//!
//! ## The pattern
//!
//! Counters that exist only in some builds used to sit as individually
//! `#[cfg]`-gated fields on the struct they measured, which forced a gate
//! at every write site too — `LayoutScratch` carried three such fields and
//! `DamageEngine` two, and the write sites needed a `#[cfg]` block each,
//! sometimes an extra gated local just to split a borrow.
//!
//! Instead, one probe struct owns the gated fields and exposes
//! **unconditional** mutators whose bodies are gated. Production call sites
//! are plain method calls; in a build without the counters the methods have
//! empty bodies and the struct is zero-sized, so they compile away. The
//! only remaining `#[cfg]`s are on the fields themselves and the read
//! accessors — which is the one placement the crate's convention allows,
//! since a struct field's gate cannot move anywhere else.
//!
//! Peer: [`crate::scene::damage::probe::DamageProbe`].
//!
//! ## Why this one is `cfg(test)` and damage's is not
//!
//! `DamageProbe` is `cfg(any(test, feature = "internals"))` because benches
//! read its counters. This one must stay test-only: [`Self::cache_hit`]
//! pushes to a `Vec`, and the `alloc_free` bench — which requires
//! `internals` and asserts steady-state frames allocate nothing — would
//! start measuring this probe's allocation instead of the frame's.

use crate::primitives::widget_id::WidgetId;

/// Per-`run` tally of [`LayoutEngine::replay_arranged`] outcomes.
///
/// [`LayoutEngine::replay_arranged`]: crate::layout::engine::LayoutEngine
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReplayCounts {
    /// Slot unchanged — the subtree's rects were copied verbatim.
    pub(crate) copied: u32,
    /// Slot moved without resizing — rects were copied and shifted.
    pub(crate) translated: u32,
}

/// What the layout pass did this `run`, for tests to assert against.
///
/// Reset by `LayoutScratch::resize_for` at the top of every run, so every
/// count is per-run rather than cumulative.
#[derive(Debug, Default)]
pub(crate) struct LayoutProbe {
    /// `intrinsic::compute` (cache-miss) calls this run. Tests assert a
    /// localized change doesn't trigger a whole-tree intrinsic re-walk.
    #[cfg(test)]
    intrinsic_computes: u32,
    /// Subtree roots restored from the measure cache this run. A cache-hit
    /// test that asserts only "warm rects equal cold rects" passes
    /// vacuously if the lookup never hit, so tests assert *where* it hit.
    #[cfg(test)]
    cache_hits: Vec<WidgetId>,
    /// Which branch `replay_arranged` took. The translate branch in
    /// particular is easy to write a fixture that silently never reaches.
    #[cfg(test)]
    arrange_replays: ReplayCounts,
}

impl LayoutProbe {
    /// Clear every counter for a new run. Retains `cache_hits` capacity so
    /// a test build doesn't reallocate each frame.
    #[inline]
    pub(crate) fn begin_run(&mut self) {
        #[cfg(test)]
        {
            self.intrinsic_computes = 0;
            self.cache_hits.clear();
            self.arrange_replays = ReplayCounts::default();
        }
    }

    #[inline]
    pub(crate) fn intrinsic_computed(&mut self) {
        #[cfg(test)]
        {
            self.intrinsic_computes += 1;
        }
    }

    #[inline]
    pub(crate) fn cache_hit(&mut self, #[allow(unused_variables)] widget: WidgetId) {
        #[cfg(test)]
        {
            self.cache_hits.push(widget);
        }
    }

    #[inline]
    pub(crate) fn arrange_copied(&mut self) {
        #[cfg(test)]
        {
            self.arrange_replays.copied += 1;
        }
    }

    #[inline]
    pub(crate) fn arrange_translated(&mut self) {
        #[cfg(test)]
        {
            self.arrange_replays.translated += 1;
        }
    }
}

/// Reads are test-only: nothing in a shipping build has a reason to ask,
/// and gating them here is what lets the fields themselves be absent.
#[cfg(test)]
impl LayoutProbe {
    pub(crate) fn intrinsic_computes(&self) -> u32 {
        self.intrinsic_computes
    }

    /// Zero the intrinsic counter mid-run — for a test that primes a frame
    /// and then counts only what a subsequent query costs.
    pub(crate) fn reset_intrinsic_computes(&mut self) {
        self.intrinsic_computes = 0;
    }

    pub(crate) fn cache_hits(&self) -> &[WidgetId] {
        &self.cache_hits
    }

    pub(crate) fn arrange_replays(&self) -> ReplayCounts {
        self.arrange_replays
    }
}
