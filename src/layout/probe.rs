//! Build-gated observability for the layout pass.
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
//! ## Mixed gates inside this struct
//!
//! Most fields are `cfg(test)`; [`Self::phase_timings`] is
//! `cfg(feature = "internals")` because benches are its only reader.
//! Per-field gates are what let one struct serve both without widening
//! either — see below for why widening the test ones would be wrong.
//!
//! ## Why the counters are `cfg(test)` and damage's are not
//!
//! `DamageProbe` is `cfg(any(test, feature = "internals"))` because benches
//! read its counters. This one must stay test-only: [`Self::cache_hit`]
//! pushes to a `Vec`, and the `alloc_free` bench — which requires
//! `internals` and asserts steady-state frames allocate nothing — would
//! start measuring this probe's allocation instead of the frame's.

use crate::primitives::widget_id::WidgetId;

/// Measure / arrange split for one `LayoutEngine::run`.
///
/// `internals`-gated rather than test-gated: the `caches` bench is the
/// only consumer, and the four clock reads per root per frame are no
/// longer negligible against the pass they measure — arrange replay took
/// the cached layout pass to ~4 µs, so the instrumentation would be a low
/// single-digit percentage of it, landing inside the frame but outside
/// the spans it reports.
#[cfg(feature = "internals")]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PhaseTimings {
    pub(crate) measure_ns: u64,
    pub(crate) arrange_ns: u64,
}

/// An open timing span. Zero-sized and free outside `internals`, so the
/// call sites need no `#[cfg]` around the `let` that opens one — which is
/// the whole reason this exists rather than a bare `Instant`.
///
/// Deliberately borrows nothing: [`Self::start`] is a free constructor,
/// so a span stays open across the `&mut self` call it is timing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseSpan {
    #[cfg(feature = "internals")]
    at: std::time::Instant,
}

impl PhaseSpan {
    #[inline]
    pub(crate) fn start() -> Self {
        Self {
            #[cfg(feature = "internals")]
            at: std::time::Instant::now(),
        }
    }

    #[cfg(feature = "internals")]
    #[inline]
    fn elapsed_ns(self) -> u64 {
        self.at.elapsed().as_nanos() as u64
    }
}

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

/// What the layout pass did this `run`, for tests and benches to assert
/// against.
///
/// Reset by [`Self::begin_run`] once at the top of every run — not in
/// `LayoutScratch::resize_for`, which runs per layer and would wipe an
/// earlier layer's counts.
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
    /// Measure / arrange wall time this run. Gated `internals` rather
    /// than `test` because benches are its only reader — see
    /// [`PhaseTimings`], and the module doc for why the two gates in this
    /// struct differ.
    #[cfg(feature = "internals")]
    phase_timings: PhaseTimings,
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
        #[cfg(feature = "internals")]
        {
            self.phase_timings = PhaseTimings::default();
        }
    }

    /// Fold a closed measure span into this run's total. Accumulates
    /// rather than assigns — `run` opens one span per root per layer.
    #[inline]
    pub(crate) fn add_measure(&mut self, #[allow(unused_variables)] span: PhaseSpan) {
        #[cfg(feature = "internals")]
        {
            self.phase_timings.measure_ns += span.elapsed_ns();
        }
    }

    /// Arrange counterpart of [`Self::add_measure`].
    #[inline]
    pub(crate) fn add_arrange(&mut self, #[allow(unused_variables)] span: PhaseSpan) {
        #[cfg(feature = "internals")]
        {
            self.phase_timings.arrange_ns += span.elapsed_ns();
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

/// Bench-facing read. Separate from the test-only accessors below
/// because its field carries the other gate.
#[cfg(feature = "internals")]
impl LayoutProbe {
    pub(crate) fn phase_timings(&self) -> PhaseTimings {
        self.phase_timings
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

#[cfg(test)]
mod tests {
    use crate::layout::probe::{LayoutProbe, PhaseSpan};

    /// The pattern's premise: with its gate off, a probe type costs
    /// nothing, so the unconditional call sites in `LayoutEngine::run`
    /// compile away rather than merely being cheap.
    ///
    /// Asserted in both configurations so the pin can't pass vacuously —
    /// a plain `cargo test` exercises the zero-sized arm, `--all-features`
    /// the populated one.
    #[test]
    fn phase_span_costs_nothing_when_its_gate_is_off() {
        #[cfg(not(feature = "internals"))]
        assert_eq!(
            size_of::<PhaseSpan>(),
            0,
            "PhaseSpan must vanish without `internals`, or every `run` pays for a clock read",
        );
        #[cfg(feature = "internals")]
        assert!(
            size_of::<PhaseSpan>() > 0,
            "with `internals` a span must actually carry an Instant",
        );
    }

    /// Same premise for the probe itself. Under `cfg(test)` its counters
    /// exist, so this pins the populated direction; the zero-sized case is
    /// unobservable from a test build by construction, which is precisely
    /// why the `PhaseSpan` pin above matters.
    #[test]
    fn probe_carries_its_counters_in_a_test_build() {
        assert!(
            size_of::<LayoutProbe>() > 0,
            "test builds must actually collect, or every assertion on the probe is vacuous",
        );
    }
}
