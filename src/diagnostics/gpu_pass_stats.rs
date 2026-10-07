//! Shared handle for the latest GPU instrumentation sample, written by the backend
//! and read by the overlay and benches through clones of one `Rc<RefCell<_>>`. Each
//! value is set independently as feature support permits.

use std::cell::RefCell;
use std::rc::Rc;
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
    pub const COUNT: usize = 10;

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
    /// Compute workgroup invocations; zero for Palantir's own render passes.
    pub compute_shader_invocations: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Inner {
    pass_ns: Option<u64>,
    kind_ns: [Option<u64>; BatchKind::COUNT],
    stats: Option<PipelineStats>,
    copy_out_ns: Option<u64>,
    main_pass_cpu_ns: Option<u64>,
}

/// Shared GPU-stats handle; clones share the latest sample, `None` until the first
/// readback.
#[derive(Clone, Debug, Default)]
pub struct GpuPassStats {
    inner: Rc<RefCell<Inner>>,
}

impl GpuPassStats {
    /// Whole-pass duration; `None` until the first resolve or without
    /// `TIMESTAMP_QUERY`.
    pub fn last_pass(&self) -> Option<Duration> {
        self.inner.borrow().pass_ns.map(Duration::from_nanos)
    }

    /// Per-category duration; `None` without `TIMESTAMP_QUERY_INSIDE_PASSES` or if
    /// the category did not run last frame.
    pub fn last_kind(&self, kind: BatchKind) -> Option<Duration> {
        self.inner.borrow().kind_ns[kind.idx()].map(Duration::from_nanos)
    }

    /// Pipeline-statistics counters around the main pass; `None` without
    /// `PIPELINE_STATISTICS_QUERY`.
    pub fn last_pipeline_stats(&self) -> Option<PipelineStats> {
        self.inner.borrow().stats
    }

    /// Backbuffer-to-target copy after the main pass in the last frame that
    /// painted; `None` if it copied nothing or `TIMESTAMP_QUERY_INSIDE_ENCODERS` is
    /// unavailable.
    pub fn last_copy_out(&self) -> Option<Duration> {
        self.inner.borrow().copy_out_ns.map(Duration::from_nanos)
    }

    /// Host CPU time the last frame spent recording the main render pass; no
    /// adapter feature gates it, and it scales with draw-step count, not pixels.
    pub fn last_main_pass_cpu(&self) -> Option<Duration> {
        self.inner
            .borrow()
            .main_pass_cpu_ns
            .map(Duration::from_nanos)
    }

    pub(crate) fn record_pass_ns(&self, ns: u64) {
        self.inner.borrow_mut().pass_ns = Some(ns);
    }

    pub(crate) fn record_main_pass_cpu_ns(&self, ns: u64) {
        self.inner.borrow_mut().main_pass_cpu_ns = Some(ns);
    }

    pub(crate) fn record_kind_ns(&self, kind: BatchKind, ns: u64) {
        self.inner.borrow_mut().kind_ns[kind.idx()] = Some(ns);
    }

    pub(crate) fn clear_kinds(&self) {
        self.inner.borrow_mut().kind_ns = [None; BatchKind::COUNT];
    }

    pub(crate) fn record_pipeline_stats(&self, stats: PipelineStats) {
        self.inner.borrow_mut().stats = Some(stats);
    }

    pub(crate) fn record_copy_out_ns(&self, ns: Option<u64>) {
        self.inner.borrow_mut().copy_out_ns = ns;
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

    #[test]
    fn handle_clones_share_state() {
        let a = GpuPassStats::default();
        let b = a.clone();
        a.record_pass_ns(3_500_000);
        a.record_main_pass_cpu_ns(250_000);
        assert_eq!(b.last_pass(), Some(Duration::from_micros(3500)));
        assert_eq!(b.last_main_pass_cpu(), Some(Duration::from_micros(250)));
    }

    #[test]
    fn record_overrides_previous_value() {
        let s = GpuPassStats::default();
        s.record_pass_ns(1_000_000);
        s.record_pass_ns(5_000_000);
        assert_eq!(s.last_pass(), Some(Duration::from_millis(5)));
        s.record_main_pass_cpu_ns(80_000);
        s.record_main_pass_cpu_ns(20_000);
        assert_eq!(s.last_main_pass_cpu(), Some(Duration::from_micros(20)));
    }

    #[test]
    fn per_kind_independent_of_total() {
        let s = GpuPassStats::default();
        s.record_kind_ns(BatchKind::Quads, 1_500_000);
        s.record_kind_ns(BatchKind::Text, 500_000);
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
        let s = GpuPassStats::default();
        let stats = PipelineStats {
            vertex_shader_invocations: 1,
            clipper_invocations: 2,
            clipper_primitives_out: 3,
            fragment_shader_invocations: 4,
            compute_shader_invocations: 0,
        };
        s.record_kind_ns(BatchKind::Quads, 2_000_000);
        s.record_kind_ns(BatchKind::Text, 1_000_000);
        s.record_pass_ns(3_000_000);
        s.record_main_pass_cpu_ns(500_000);
        s.record_pipeline_stats(stats);
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

    #[test]
    fn pipeline_stats_round_trip() {
        let s = GpuPassStats::default();
        s.record_pipeline_stats(PipelineStats {
            vertex_shader_invocations: 1,
            clipper_invocations: 2,
            clipper_primitives_out: 3,
            fragment_shader_invocations: 4,
            compute_shader_invocations: 0,
        });
        let v = s.last_pipeline_stats().expect("recorded");
        assert_eq!(v.vertex_shader_invocations, 1);
        assert_eq!(v.clipper_invocations, 2);
        assert_eq!(v.clipper_primitives_out, 3);
        assert_eq!(v.fragment_shader_invocations, 4);
        assert_eq!(v.compute_shader_invocations, 0);
    }
}
