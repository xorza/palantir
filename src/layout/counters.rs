//! Build-gated observability for the layout pass, on [`TestOnly`]; [`PhaseTimings`]
//! rides a [`BenchOnly`] cell and [`PhaseSpan::elapsed_ns`] answers zero without
//! `bench`.

use crate::common::counters::{BenchOnly, TestOnly, counter_snapshot};
use crate::primitives::identity::widget_id::WidgetId;

/// CPU nanoseconds one `LayoutEngine::run` spent in each layout phase, summed over
/// every root in every layer. Split because the cross-frame cache covers only
/// measure; a whole-`run` number would average away that asymmetry. The clock reads
/// are `bench`-gated in [`PhaseSpan`], so a test build accumulates zeros.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PhaseTimings {
    pub(crate) measure_ns: u64,
    pub(crate) arrange_ns: u64,
    pub(crate) capture_ns: u64,
}

/// An open timing span. Zero-sized and free outside `bench`, so call sites need no
/// `#[cfg]`. Borrows nothing, so it stays open across the `&mut self` call it
/// times.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseSpan {
    #[cfg(feature = "bench")]
    at: std::time::Instant,
}

impl PhaseSpan {
    #[inline]
    #[cfg_attr(
        not(feature = "bench"),
        expect(
            clippy::missing_const_for_fn,
            reason = "with `bench` the body reads the clock"
        )
    )]
    pub(crate) fn start() -> Self {
        Self {
            #[cfg(feature = "bench")]
            at: std::time::Instant::now(),
        }
    }

    /// Nanoseconds since the span opened; zero without `bench`.
    #[inline]
    #[cfg_attr(
        not(feature = "bench"),
        expect(
            clippy::missing_const_for_fn,
            reason = "with `bench` the body reads the clock"
        )
    )]
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

    /// Which branch [`LayoutPass::replay_arranged`] took.
    ///
    /// [`LayoutPass::replay_arranged`]: crate::layout::pass::LayoutPass
    pub(crate) struct ReplayCounters;

    pub(crate) struct ReplayCounts;

    /// Slot unchanged: rects copied verbatim.
    copied: u32,
    /// Slot moved without resizing: rects copied and shifted.
    translated: u32,
}

/// What the layout pass did this `run`. Reset by [`Self::begin_pass`] once per run,
/// not in `LayoutScratch::resize_for`, which runs per layer.
#[derive(Debug, Default)]
pub(crate) struct LayoutCounters {
    intrinsic_computes: TestOnly<u32>,
    /// Subtree roots restored from the measure cache this run, so tests can assert
    /// *where* it hit.
    cache_hits: TestOnly<Vec<WidgetId>>,
    kept_runs: TestOnly<u32>,
    replays: ReplayCounters,
    phase_timings: BenchOnly<PhaseTimings>,
}

impl LayoutCounters {
    #[inline]
    pub(crate) fn begin_pass(&mut self) {
        self.intrinsic_computes.reset();
        self.cache_hits.clear();
        self.replays.copied.reset();
        self.replays.translated.reset();
        self.phase_timings.reset();
    }

    #[inline]
    pub(crate) fn add_measure(&mut self, span: PhaseSpan) {
        self.phase_timings
            .edit(|t| t.measure_ns += span.elapsed_ns());
    }

    /// Snapshot capture ([`MeasureCache::capture_tree`] plus
    /// [`MeasureCache::end_frame`]), which runs outside the measure and arrange
    /// spans.
    ///
    /// [`MeasureCache::capture_tree`]: crate::layout::cache::MeasureCache
    /// [`MeasureCache::end_frame`]: crate::layout::cache::MeasureCache
    #[inline]
    pub(crate) fn add_capture(&mut self, span: PhaseSpan) {
        self.phase_timings
            .edit(|t| t.capture_ns += span.elapsed_ns());
    }

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
    pub(crate) fn kept_last_run(&mut self) {
        self.kept_runs.bump();
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

/// Reads gated to the builds that ask: `bench.rs` reads the phase timings, tests
/// the rest.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    use crate::layout::counters::LayoutCounters;
    #[cfg(feature = "bench")]
    use crate::layout::counters::PhaseTimings;
    #[cfg(test)]
    use crate::layout::counters::ReplayCounts;
    #[cfg(test)]
    use crate::primitives::identity::widget_id::WidgetId;

    impl LayoutCounters {
        #[cfg(feature = "bench")]
        pub(crate) const fn phase_timings(&self) -> PhaseTimings {
            *self.phase_timings.get()
        }

        #[cfg(test)]
        pub(crate) const fn intrinsic_computes(&self) -> u32 {
            self.intrinsic_computes.count()
        }

        #[cfg(test)]
        pub(crate) fn reset_intrinsic_computes(&mut self) {
            self.intrinsic_computes.reset();
        }

        #[cfg(test)]
        pub(crate) const fn kept_runs(&self) -> u32 {
            self.kept_runs.count()
        }

        #[cfg(test)]
        pub(crate) fn cache_hits(&self) -> &[WidgetId] {
            self.cache_hits.as_slice()
        }

        #[cfg(test)]
        pub(crate) fn arrange_replays(&self) -> ReplayCounts {
            self.replays.counts()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::counters::{LayoutCounters, PhaseSpan};

    /// With its gate off a probe type costs nothing, so the unconditional call
    /// sites in `LayoutEngine::run` compile away. Asserted in both configurations
    /// so the pin cannot pass vacuously.
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

    /// Same premise for the probe; under `cfg(test)` only the populated direction
    /// is observable.
    #[test]
    fn counters_are_carried_only_in_a_test_build() {
        assert!(
            size_of::<LayoutCounters>() > 0,
            "test builds must actually collect, or every assertion on the probe is vacuous",
        );
    }
}
