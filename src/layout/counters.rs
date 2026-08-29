//! Build-gated observability for the layout pass. Built on [`TestOnly`],
//! whose module doc explains the gated-cell pattern and why the gates
//! differ between passes.
//!
//! Test-only rather than the wider gate because
//! [`LayoutCounters::cache_hits`] pushes to a `Vec` on *every* cache hit,
//! which in steady state is every subtree root — the alloc bench requires
//! `bench` and asserts steady-state frames allocate nothing, so it would
//! end up measuring this probe instead of the frame.
//!
//! [`PhaseTimings`] rides a [`BenchOnly`] cell like everything else
//! here. What lets it is [`PhaseSpan::elapsed_ns`] answering zero
//! without the `bench` feature rather than not existing: the clock read
//! is what a build must not pay for, and that is gone either way, so the
//! mutator needs no `#[cfg]` of its own.

use crate::common::counters::{BenchOnly, TestOnly, counter_snapshot};
use crate::primitives::widget_id::WidgetId;

/// CPU nanoseconds one `LayoutEngine::run` spent in each half of the
/// layout pass, summed over every root in every layer.
///
/// Split because the cross-frame cache covers only the first half.
/// `MeasureCache::try_lookup` can short-circuit an entire subtree — in
/// steady state the root itself, so measure collapses to a few whole-tree
/// `copy_from_slice`s — while `LayoutEngine::arrange` walks every node
/// with full driver dispatch regardless. A whole-`run` number averages
/// that asymmetry away; these two are what make it visible.
///
/// The sliver between the two (resolving the root's own size from
/// `desired`) is charged to neither: it is one `arrange_size` call per
/// root, independent of tree size.
///
/// The `caches` bench is the only consumer, and the four clock reads per
/// root per frame are no longer negligible against the pass they measure
/// — arrange replay took the cached layout pass to ~4 µs, so the
/// instrumentation would be a low single-digit percentage of it, landing
/// inside the frame but outside the spans it reports. So the reads
/// themselves are `bench`-gated, in [`PhaseSpan`], and what a test build
/// accumulates here is zeros.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PhaseTimings {
    pub(crate) measure_ns: u64,
    pub(crate) arrange_ns: u64,
    pub(crate) capture_ns: u64,
}

/// An open timing span. Zero-sized and free outside `bench`, so the
/// call sites need no `#[cfg]` around the `let` that opens one — which is
/// the whole reason this exists rather than a bare `Instant`.
///
/// Deliberately borrows nothing: [`Self::start`] is a free constructor,
/// so a span stays open across the `&mut self` call it is timing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseSpan {
    #[cfg(feature = "bench")]
    at: std::time::Instant,
}

impl PhaseSpan {
    #[inline]
    pub(crate) fn start() -> Self {
        Self {
            #[cfg(feature = "bench")]
            at: std::time::Instant::now(),
        }
    }

    /// Nanoseconds since the span opened, and zero without `bench` —
    /// where there is no reading to subtract from.
    ///
    /// Answering rather than not existing is what lets
    /// [`LayoutCounters`]'s timing mutators be plain cell edits: a
    /// closure body typechecks whether or not it runs.
    #[inline]
    fn elapsed_ns(self) -> u64 {
        #[cfg(feature = "bench")]
        {
            self.at.elapsed().as_nanos() as u64
        }
        #[cfg(not(feature = "bench"))]
        {
            0
        }
    }
}

counter_snapshot! {
    cells TestOnly, reads cfg(test);

    /// Which branch [`LayoutEngine::replay_arranged`] took. The translate
    /// branch in particular is easy to write a fixture that silently
    /// never reaches.
    ///
    /// [`LayoutEngine::replay_arranged`]: crate::layout::engine::LayoutEngine
    pub(crate) struct ReplayCounters;

    /// One reading of a [`ReplayCounters`]. Tests compare it as a
    /// literal.
    pub(crate) struct ReplayCounts;

    /// Slot unchanged — the subtree's rects were copied verbatim.
    copied: u32,
    /// Slot moved without resizing — rects were copied and shifted.
    translated: u32,
}

/// What the layout pass did this `run`, for tests and benches to assert
/// against.
///
/// Reset by [`Self::begin_run`] once at the top of every run — not in
/// `LayoutScratch::resize_for`, which runs per layer and would wipe an
/// earlier layer's counts.
#[derive(Debug, Default)]
pub(crate) struct LayoutCounters {
    /// `intrinsic::compute` (cache-miss) calls this run. Tests assert a
    /// localized change doesn't trigger a whole-tree intrinsic re-walk.
    intrinsic_computes: TestOnly<u32>,
    /// Subtree roots restored from the measure cache this run. A cache-hit
    /// test that asserts only "warm rects equal cold rects" passes
    /// vacuously if the lookup never hit, so tests assert *where* it hit.
    cache_hits: TestOnly<Vec<WidgetId>>,
    replays: ReplayCounters,
    /// Measure / arrange wall time this run.
    phase_timings: BenchOnly<PhaseTimings>,
}

impl LayoutCounters {
    /// Clear every counter for a new pass. Retains `cache_hits` capacity so
    /// a test build doesn't reallocate each frame.
    #[inline]
    pub(crate) fn begin_pass(&mut self) {
        self.intrinsic_computes.reset();
        self.cache_hits.clear();
        self.replays.copied.reset();
        self.replays.translated.reset();
        self.phase_timings.reset();
    }

    /// Fold a closed measure span into this run's total. Accumulates
    /// rather than assigns — `run` opens one span per root per layer.
    #[inline]
    pub(crate) fn add_measure(&mut self, span: PhaseSpan) {
        self.phase_timings
            .edit(|t| t.measure_ns += span.elapsed_ns());
    }

    /// Snapshot capture — [`MeasureCache::capture_tree`] plus
    /// [`MeasureCache::end_frame`]. The third layout phase, and the
    /// easiest to miss: both run outside the measure and arrange spans,
    /// so without this counter a frame's layout time reads short by
    /// however much the snapshot cost.
    ///
    /// It is the phase whose *share* grows as the cache works better —
    /// capture is O(nodes) on any changed frame, while measure shrinks
    /// to the subtrees that actually missed. `broad/measure/localized`
    /// is the shape that makes that visible.
    ///
    /// [`MeasureCache::capture_tree`]: crate::layout::cache::MeasureCache
    /// [`MeasureCache::end_frame`]: crate::layout::cache::MeasureCache
    #[inline]
    pub(crate) fn add_capture(&mut self, span: PhaseSpan) {
        self.phase_timings
            .edit(|t| t.capture_ns += span.elapsed_ns());
    }

    /// Arrange counterpart of [`Self::add_measure`].
    #[inline]
    pub(crate) fn add_arrange(&mut self, span: PhaseSpan) {
        self.phase_timings
            .edit(|t| t.arrange_ns += span.elapsed_ns());
    }

    #[inline]
    pub(crate) fn intrinsic_computed(&mut self) {
        self.intrinsic_computes.bump();
    }

    #[inline]
    pub(crate) fn cache_hit(&mut self, widget: WidgetId) {
        self.cache_hits.push(widget);
    }

    #[inline]
    pub(crate) fn arrange_copied(&mut self) {
        self.replays.copied.bump();
    }

    #[inline]
    pub(crate) fn arrange_translated(&mut self) {
        self.replays.translated.bump();
    }
}

/// Bench-facing read. Separate from the test-only accessors below
/// because its cell carries the wider gate.
#[cfg(feature = "bench")]
impl LayoutCounters {
    pub(crate) fn phase_timings(&self) -> PhaseTimings {
        *self.phase_timings.get()
    }
}

/// Reads are test-only: nothing in a shipping build has a reason to ask,
/// and gating them here is what lets the counters themselves be absent.
#[cfg(test)]
impl LayoutCounters {
    pub(crate) fn intrinsic_computes(&self) -> u32 {
        self.intrinsic_computes.count()
    }

    /// Zero the intrinsic counter mid-run — for a test that primes a frame
    /// and then counts only what a subsequent query costs.
    pub(crate) fn reset_intrinsic_computes(&mut self) {
        self.intrinsic_computes.reset();
    }

    pub(crate) fn cache_hits(&self) -> &[WidgetId] {
        self.cache_hits.as_slice()
    }

    pub(crate) fn arrange_replays(&self) -> ReplayCounts {
        // Full path rather than a `use`: `CounterSet` is itself gated, so
        // an import of it at the top of the file would not compile in the
        // builds this accessor is absent from.
        crate::common::counters::CounterSet::counts(&self.replays)
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::counters::{LayoutCounters, PhaseSpan};

    /// The pattern's premise: with its gate off, a probe type costs
    /// nothing, so the unconditional call sites in `LayoutEngine::run`
    /// compile away rather than merely being cheap.
    ///
    /// Asserted in both configurations so the pin can't pass vacuously —
    /// a plain `cargo test` exercises the zero-sized arm, `--all-features`
    /// the populated one.
    #[test]
    fn phase_span_costs_nothing_when_its_gate_is_off() {
        #[cfg(not(feature = "bench"))]
        assert_eq!(
            size_of::<PhaseSpan>(),
            0,
            "PhaseSpan must vanish without `bench`, or every `run` pays for a clock read",
        );
        #[cfg(feature = "bench")]
        assert!(
            size_of::<PhaseSpan>() > 0,
            "with `bench` a span must actually carry an Instant",
        );
    }

    /// Same premise for the probe itself. Under `cfg(test)` its counters
    /// exist, so this pins the populated direction; the zero-sized case is
    /// unobservable from a test build by construction, which is precisely
    /// why the `PhaseSpan` pin above matters.
    #[test]
    fn counters_are_carried_only_in_a_test_build() {
        assert!(
            size_of::<LayoutCounters>() > 0,
            "test builds must actually collect, or every assertion on the probe is vacuous",
        );
    }
}
