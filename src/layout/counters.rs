//! Build-gated observability for the layout pass, on [`TestOnly`].

use crate::common::counters::{TestOnly, counter_snapshot};
use crate::primitives::identity::widget_id::WidgetId;

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
}

impl LayoutCounters {
    #[inline]
    pub(crate) fn begin_pass(&mut self) {
        self.intrinsic_computes.reset();
        self.cache_hits.clear();
        self.replays.copied.reset();
        self.replays.translated.reset();
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

/// The probe's reads, for the crate's tests.
#[cfg(test)]
pub(crate) mod internals {
    use crate::common::counters::CounterSet;
    use crate::layout::counters::LayoutCounters;
    use crate::layout::counters::ReplayCounts;
    use crate::primitives::identity::widget_id::WidgetId;

    impl LayoutCounters {
        pub(crate) const fn intrinsic_computes(&self) -> u32 {
            self.intrinsic_computes.count()
        }

        pub(crate) fn reset_intrinsic_computes(&mut self) {
            self.intrinsic_computes.reset();
        }

        pub(crate) const fn kept_runs(&self) -> u32 {
            self.kept_runs.count()
        }

        pub(crate) fn cache_hits(&self) -> &[WidgetId] {
            self.cache_hits.as_slice()
        }

        pub(crate) fn arrange_replays(&self) -> ReplayCounts {
            self.replays.counts()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::counters::LayoutCounters;

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
