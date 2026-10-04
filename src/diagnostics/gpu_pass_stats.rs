//! Owned, sharable container for the most recent GPU instrumentation
//! sample. Backend writes; Ui (debug overlay) and benches read; all
//! parties hold a `Clone` of the same `Rc<RefCell<_>>` handle so the
//! reader sees the writer's latest publish without a global static.
//!
//! Four kinds of data, set independently as feature support permits:
//!
//! - **Whole-pass duration** ([`GpuPassStats::last_pass_ms`]). Always
//!   populated when `TIMESTAMP_QUERY` is on.
//! - **Per-batch-kind duration** ([`GpuPassStats::last_kind_ms`]).
//!   Populated when `TIMESTAMP_QUERY_INSIDE_PASSES` is on.
//! - **Pipeline statistics** ([`GpuPassStats::last_pipeline_stats`]).
//!   Populated when `PIPELINE_STATISTICS_QUERY` is on.
//! - **Main-pass CPU record time**
//!   ([`GpuPassStats::last_main_pass_cpu_ms`]). The odd one out: host-side,
//!   not device-side, so it needs no adapter feature and no opt-in and is
//!   populated on every submitted frame.
//!
//! Two producers, both on the host thread: the backend's
//! `GpuTimings::after_submit` publishes the three device-side values,
//! `WgpuBackend::run_main_pass` publishes the CPU one. Many readers (debug
//! overlay, benches). `RefCell` is sufficient and panics on the
//! caller-bug case of a concurrent borrow.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Categories of work the per-batch timestamp marker distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchKind {
    /// Setup work between the pass beginning and the first drawing
    /// step (uniform binds, scissor sets, stencil-ref before the first
    /// mask quad). Useful as a sanity baseline — should be near 0.
    Setup = 0,
    /// `RenderStep::PreClear` — the per-rect clear-color quad emitted
    /// at the start of each Partial pass.
    PreClear = 1,
    /// `RenderStep::MaskStamp` / `MaskClear` — stencil mask quads.
    Mask = 2,
    /// `RenderStep::Quads` — the main quad pipeline.
    Quads = 3,
    /// `RenderStep::Text` — text batches via the inlined text
    /// pipeline.
    Text = 4,
    /// `PaintTier::Mesh`'s replay — the mesh pipeline.
    Mesh = 5,
    /// `PaintTier::Image`'s replay — the image pipeline.
    Image = 6,
    /// `PaintTier::Curve`'s replay — the curve pipeline.
    Curve = 7,
    /// `PaintTier::Icon`'s replay — the icon pipeline (the glyph shader
    /// over the icon atlas).
    Icon = 8,
}

impl BatchKind {
    /// How many kinds there are.
    pub const COUNT: usize = 9;

    /// Every kind, in discriminant order — the order a reporter lists
    /// them in.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Setup,
        Self::PreClear,
        Self::Mask,
        Self::Quads,
        Self::Text,
        Self::Mesh,
        Self::Image,
        Self::Curve,
        Self::Icon,
    ];

    pub(crate) const fn idx(self) -> usize {
        self as u8 as usize
    }

    /// Human-readable label for debug overlays and bench reporters: the
    /// variant name, lowercased.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::PreClear => "preclear",
            Self::Mask => "mask",
            Self::Quads => "quads",
            Self::Text => "text",
            Self::Mesh => "mesh",
            Self::Image => "image",
            Self::Curve => "curve",
            Self::Icon => "icon",
        }
    }
}

// `ALL` lists every discriminant once, in order, so `idx` indexes it and
// a `[_; COUNT]` table keyed by `idx` has a slot per kind. A new variant
// fails `label`'s match first, and this second.
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

/// Counters surfaced by [`GpuPassStats::last_pipeline_stats`]. Order
/// matches `wgpu::PipelineStatisticsTypes`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Vertices the vertex stage ran on.
    pub vertex_shader_invocations: u64,
    /// Primitives the clipper was handed.
    pub clipper_invocations: u64,
    /// Primitives the clipper emitted.
    pub clipper_primitives_out: u64,
    /// Fragments the fragment stage ran on.
    pub fragment_shader_invocations: u64,
    /// Compute workgroup invocations. Zero for the render passes
    /// Palantir itself submits.
    pub compute_shader_invocations: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Inner {
    pass_ns: Option<u64>,
    kind_ns: [Option<u64>; BatchKind::COUNT],
    stats: Option<PipelineStats>,
    main_pass_cpu_ns: Option<u64>,
}

/// Shared GPU-stats handle. Clone is a cheap refcount bump — every
/// holder sees the latest sample published by `GpuTimings`. Pre-first-
/// readback (or on adapters that don't advertise `TIMESTAMP_QUERY`)
/// all readers return `None`.
#[derive(Clone, Debug, Default)]
pub struct GpuPassStats {
    inner: Rc<RefCell<Inner>>,
}

impl GpuPassStats {
    /// Whole-pass duration, or `None` until the first frame's resolve has
    /// landed (or always `None` on adapters without `TIMESTAMP_QUERY` or
    /// when collection is disabled).
    pub fn last_pass(&self) -> Option<Duration> {
        self.inner.borrow().pass_ns.map(Duration::from_nanos)
    }

    /// Per-category duration. `None` when
    /// `TIMESTAMP_QUERY_INSIDE_PASSES` is unavailable / disabled, or
    /// when the named category didn't run in the most recent measured
    /// frame.
    pub fn last_kind(&self, kind: BatchKind) -> Option<Duration> {
        self.inner.borrow().kind_ns[kind.idx()].map(Duration::from_nanos)
    }

    /// Pipeline-statistics counters around the main pass. `None` when
    /// `PIPELINE_STATISTICS_QUERY` is unavailable / disabled.
    pub fn last_pipeline_stats(&self) -> Option<PipelineStats> {
        self.inner.borrow().stats
    }

    /// Host CPU time the most recent frame spent opening the main render
    /// pass, recording every draw step into it, and closing it — i.e. the
    /// wgpu command-recording cost of `WgpuBackend::run_main_pass`,
    /// including the end-of-pass command replay. Independent of every
    /// device-side value above: no adapter feature gates it, so it is
    /// `Some` after the first submitted frame on any GPU.
    ///
    /// This is what scales with the *number* of draw steps rather than the
    /// number of pixels, which makes it the metric for bind/draw-count
    /// work (batch coalescing, bind-state deduplication).
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

    /// Clears every per-kind slot back to `None`. Called before
    /// publishing a fresh frame's per-kind values so categories that
    /// didn't run this frame don't keep showing the previous frame's
    /// number.
    pub(crate) fn clear_kinds(&self) {
        self.inner.borrow_mut().kind_ns = [None; BatchKind::COUNT];
    }

    pub(crate) fn record_pipeline_stats(&self, stats: PipelineStats) {
        self.inner.borrow_mut().stats = Some(stats);
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
        // Pin: stores the *latest* sample, not a rolling EMA. The bench
        // reporters sample per frame and summarize themselves, so a
        // smoothing publisher here would silently flatten their spread.
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
        // Total isn't auto-populated from per-kind.
        assert_eq!(s.last_pass(), None);
        // Nor is the CPU record time — it has a separate producer, and a
        // device with no timestamp support publishes only that one.
        assert_eq!(s.last_main_pass_cpu(), None);
    }

    #[test]
    fn clear_kinds_resets_to_none() {
        // Pin: a category that ran last frame but not this one shows
        // `None`, not the stale previous-frame value.
        // The pass total, the CPU time and the pipeline stats are other
        // producers' values, so clearing the kinds leaves them alone.
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
