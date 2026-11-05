//! A window's latest GPU instrumentation sample, which the backend publishes into on the
//! window's submits and the overlay and benches read through the window's `Ui`. Each value is
//! set independently as feature support permits.

use std::time::Duration;

/// Categories of work the per-batch timestamp marker distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchKind {
    /// Work before the first drawing step (binds, scissor, stencil-ref).
    Setup = 0,
    /// Per-rect clear quad of a Partial pass.
    PreClear = 1,
    /// Stencil mask quads.
    Mask = 2,
    /// The quad pipeline.
    Quads = 3,
    /// Drop and inset shadows.
    Shadows = 4,
    /// Text batches.
    Text = 5,
    /// Mesh replay.
    Mesh = 6,
    /// Image replay.
    Image = 7,
    /// Curve replay.
    Curve = 8,
    /// Icon replay.
    Icon = 9,
}

impl BatchKind {
    /// How many kinds there are.
    pub(crate) const COUNT: usize = 10;

    /// Every kind, in discriminant order.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Setup,
        Self::PreClear,
        Self::Mask,
        Self::Quads,
        Self::Shadows,
        Self::Text,
        Self::Mesh,
        Self::Image,
        Self::Curve,
        Self::Icon,
    ];

    pub(crate) const fn idx(self) -> usize {
        self as u8 as usize
    }

    /// Lowercased variant name, for overlays and reporters.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::PreClear => "preclear",
            Self::Mask => "mask",
            Self::Quads => "quads",
            Self::Shadows => "shadows",
            Self::Text => "text",
            Self::Mesh => "mesh",
            Self::Image => "image",
            Self::Curve => "curve",
            Self::Icon => "icon",
        }
    }
}

// `ALL` lists every discriminant once, in order, so `idx` indexes it.
const _: () = {
    let mut i = 0;
    while i < BatchKind::COUNT {
        assert!(
            BatchKind::ALL[i].idx() == i,
            "BatchKind::ALL must list every discriminant in order",
        );
        i += 1;
    }
    assert!(BatchKind::Icon.idx() + 1 == BatchKind::COUNT);
};

/// Counters surfaced by [`GpuPassStats::last_pipeline_stats`], in
/// `wgpu::PipelineStatisticsTypes` order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Vertex stage invocations.
    pub vertex_shader_invocations: u64,
    /// Primitives the clipper was handed.
    pub clipper_invocations: u64,
    /// Primitives the clipper emitted.
    pub clipper_primitives_out: u64,
    /// Fragment stage invocations.
    pub fragment_shader_invocations: u64,
}

/// The last frame's backdrop work on the device: the sums over its breaks of their captures and
/// of their blurs, `None` when it timed none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BackdropTimes {
    pub(crate) captures: Option<Duration>,
    pub(crate) blurs: Option<Duration>,
}

/// A window's latest GPU sample, each value `None` until the first readback that sets it. A
/// clone is a snapshot.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuPassStats {
    pass: Option<Duration>,
    kinds: [Option<Duration>; BatchKind::COUNT],
    stats: Option<PipelineStats>,
    copy_out: Option<Duration>,
    backdrops: BackdropTimes,
    main_pass_cpu: Option<Duration>,
}

impl GpuPassStats {
    /// Whole-pass duration; `None` until the first resolve or without
    /// `TIMESTAMP_QUERY`. On a frame whose backdrop panes capture, the main pass
    /// breaks at each, and the duration spans the captures and blurs between.
    pub const fn last_pass(&self) -> Option<Duration> {
        self.pass
    }

    /// Per-category duration; `None` without `TIMESTAMP_QUERY_INSIDE_PASSES` or if
    /// the category did not run last frame. The captures and blurs between the
    /// passes the main pass breaks into count in no category, and a frame of more
    /// breaks than its timestamps hold publishes none.
    pub const fn last_kind(&self, kind: BatchKind) -> Option<Duration> {
        self.kinds[kind.idx()]
    }

    /// Pipeline-statistics counters around the main pass, summed over the passes
    /// it breaks into at backdrop panes; `None` without `PIPELINE_STATISTICS_QUERY`,
    /// or when the last frame broke it into more than 16 passes.
    pub const fn last_pipeline_stats(&self) -> Option<PipelineStats> {
        self.stats
    }

    /// Backbuffer-to-target copy after the main pass in the last frame that
    /// painted; `None` if it copied nothing or `TIMESTAMP_QUERY_INSIDE_ENCODERS` is
    /// unavailable.
    pub const fn last_copy_out(&self) -> Option<Duration> {
        self.copy_out
    }

    /// Host CPU time the last frame spent recording the main render pass; no
    /// adapter feature gates it, and it scales with draw-step count, not pixels.
    pub const fn last_main_pass_cpu(&self) -> Option<Duration> {
        self.main_pass_cpu
    }

    pub(crate) const fn record_pass(&mut self, time: Duration) {
        self.pass = Some(time);
    }

    pub(crate) const fn record_main_pass_cpu(&mut self, time: Duration) {
        self.main_pass_cpu = Some(time);
    }

    pub(crate) const fn record_kind(&mut self, kind: BatchKind, time: Duration) {
        self.kinds[kind.idx()] = Some(time);
    }

    pub(crate) const fn clear_kinds(&mut self) {
        self.kinds = [None; BatchKind::COUNT];
    }

    pub(crate) const fn record_pipeline_stats(&mut self, stats: Option<PipelineStats>) {
        self.stats = stats;
    }

    pub(crate) const fn record_copy_out(&mut self, time: Option<Duration>) {
        self.copy_out = time;
    }

    #[cfg_attr(
        not(feature = "internals"),
        expect(dead_code, reason = "the backdrop benches read it")
    )]
    pub(crate) const fn last_backdrops(&self) -> BackdropTimes {
        self.backdrops
    }

    pub(crate) const fn record_backdrops(&mut self, times: BackdropTimes) {
        self.backdrops = times;
    }
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::gpu_pass_stats::*;

    #[test]
    fn starts_uninit() {
        let s = GpuPassStats::default();
        assert_eq!(s.last_pass(), None);
        assert_eq!(s.last_kind(BatchKind::Quads), None);
        assert_eq!(s.last_pipeline_stats(), None);
        assert_eq!(s.last_copy_out(), None);
        assert_eq!(s.last_main_pass_cpu(), None);
    }

    /// A copy keeps the sample it was taken at: a record into the original after it reaches only
    /// the original.
    #[test]
    fn a_copy_keeps_the_sample_it_was_taken_at() {
        let mut a = GpuPassStats::default();
        a.record_pass(Duration::from_micros(3500));
        let b = a;
        a.record_pass(Duration::from_micros(10));
        a.record_main_pass_cpu(Duration::from_micros(250));
        assert_eq!(b.last_pass(), Some(Duration::from_micros(3500)));
        assert_eq!(b.last_main_pass_cpu(), None);
        assert_eq!(a.last_pass(), Some(Duration::from_micros(10)));
        assert_eq!(a.last_main_pass_cpu(), Some(Duration::from_micros(250)));
    }

    #[test]
    fn record_overrides_previous_value() {
        let mut s = GpuPassStats::default();
        s.record_pass(Duration::from_millis(1));
        s.record_pass(Duration::from_millis(5));
        assert_eq!(s.last_pass(), Some(Duration::from_millis(5)));
        s.record_main_pass_cpu(Duration::from_micros(80));
        s.record_main_pass_cpu(Duration::from_micros(20));
        assert_eq!(s.last_main_pass_cpu(), Some(Duration::from_micros(20)));
    }

    #[test]
    fn per_kind_independent_of_total() {
        let mut s = GpuPassStats::default();
        s.record_kind(BatchKind::Quads, Duration::from_micros(1500));
        s.record_kind(BatchKind::Text, Duration::from_micros(500));
        assert_eq!(
            s.last_kind(BatchKind::Quads),
            Some(Duration::from_micros(1500))
        );
        assert_eq!(
            s.last_kind(BatchKind::Text),
            Some(Duration::from_micros(500))
        );
        assert_eq!(s.last_kind(BatchKind::Mesh), None);
        assert_eq!(s.last_pass(), None);
        assert_eq!(s.last_main_pass_cpu(), None);
    }

    #[test]
    fn clear_kinds_resets_to_none() {
        let mut s = GpuPassStats::default();
        let stats = PipelineStats {
            vertex_shader_invocations: 1,
            clipper_invocations: 2,
            clipper_primitives_out: 3,
            fragment_shader_invocations: 4,
        };
        s.record_kind(BatchKind::Quads, Duration::from_millis(2));
        s.record_kind(BatchKind::Text, Duration::from_millis(1));
        s.record_pass(Duration::from_millis(3));
        s.record_main_pass_cpu(Duration::from_micros(500));
        s.record_pipeline_stats(Some(stats));
        s.clear_kinds();
        for kind in BatchKind::ALL {
            assert_eq!(s.last_kind(kind), None, "{kind:?}");
        }
        assert_eq!(s.last_pass(), Some(Duration::from_millis(3)));
        assert_eq!(s.last_main_pass_cpu(), Some(Duration::from_micros(500)));
        assert_eq!(s.last_pipeline_stats(), Some(stats));
    }

    #[test]
    fn labels_match_lowercased_variant_names() {
        for kind in BatchKind::ALL {
            assert_eq!(kind.label(), format!("{kind:?}").to_lowercase(), "{kind:?}");
        }
    }
}
