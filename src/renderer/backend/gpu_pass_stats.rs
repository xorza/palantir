//! Owned, sharable container for the most recent GPU instrumentation
//! sample. Backend writes; Ui (debug overlay) and benches read; all
//! parties hold a `Clone` of the same `Rc<RefCell<_>>` handle so the
//! reader sees the writer's latest publish without a global static.
//!
//! Three kinds of data, set independently as feature support permits:
//!
//! - **Whole-pass duration** ([`GpuPassStats::last_pass_ms`]). Always
//!   populated when `TIMESTAMP_QUERY` is on.
//! - **Per-batch-kind duration** ([`GpuPassStats::last_kind_ms`]).
//!   Populated when `TIMESTAMP_QUERY_INSIDE_PASSES` is on.
//! - **Pipeline statistics** ([`GpuPassStats::last_pipeline_stats`]).
//!   Populated when `PIPELINE_STATISTICS_QUERY` is on.
//!
//! Single-producer (the backend's `GpuTimings::after_submit`),
//! many-reader (debug overlay, benches), all on the same thread
//! (Host owns both sides). `RefCell` is sufficient and panics on the
//! caller-bug case of a concurrent borrow.

use std::cell::RefCell;
use std::rc::Rc;
use strum::{EnumCount, EnumIter, IntoStaticStr};

/// Categories of work the per-batch timestamp marker distinguishes.
/// `IntoStaticStr` with `serialize_all = "lowercase"` powers
/// [`Self::label`] — `PreClear` → `"preclear"`, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumCount, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
#[repr(u8)]
pub enum BatchKind {
    /// Setup work between the pass beginning and the first drawing
    /// step (uniform binds, scissor sets, stencil-ref before the first
    /// `MaskQuad`). Useful as a sanity baseline — should be near 0.
    Setup = 0,
    /// `RenderStep::PreClear` — the per-rect clear-color quad emitted
    /// at the start of each Partial pass.
    PreClear = 1,
    /// `RenderStep::MaskQuad` — stencil mask write/clear quads.
    Mask = 2,
    /// `RenderStep::Quads` — the main quad pipeline.
    Quads = 3,
    /// `RenderStep::Text` — text batches via the inlined glyphon
    /// pipeline.
    Text = 4,
    /// `RenderStep::MeshBatch` — the mesh pipeline.
    Mesh = 5,
    /// `RenderStep::ImageBatch` — the image pipeline.
    Image = 6,
    /// `RenderStep::CurveBatch` — the curve pipeline.
    Curve = 7,
}

impl BatchKind {
    pub(crate) fn idx(self) -> usize {
        self as u8 as usize
    }

    /// Human-readable label for debug overlays / bench reporters.
    /// Lowercased variant name via `strum::IntoStaticStr` — adding a
    /// new variant carries its label automatically.
    pub fn label(self) -> &'static str {
        self.into()
    }
}

/// Counters surfaced by [`GpuPassStats::last_pipeline_stats`]. Order
/// matches `wgpu::PipelineStatisticsTypes`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineStats {
    pub vertex_shader_invocations: u64,
    pub clipper_invocations: u64,
    pub clipper_primitives_out: u64,
    pub fragment_shader_invocations: u64,
    pub compute_shader_invocations: u64,
}

#[derive(Clone, Copy, Default)]
struct Inner {
    pass_ns: Option<u64>,
    kind_ns: [Option<u64>; <BatchKind as strum::EnumCount>::COUNT],
    stats: Option<PipelineStats>,
}

/// Shared GPU-stats handle. Clone is a cheap refcount bump — every
/// holder sees the latest sample published by `GpuTimings`. Pre-first-
/// readback (or on adapters that don't advertise `TIMESTAMP_QUERY`)
/// all readers return `None`.
#[derive(Clone, Default)]
pub struct GpuPassStats {
    inner: Rc<RefCell<Inner>>,
}

impl GpuPassStats {
    /// Whole-pass duration in milliseconds, or `None` until the first
    /// frame's resolve has landed (or always `None` on adapters
    /// without `TIMESTAMP_QUERY` / when `collect_gpu_stats` is off).
    pub fn last_pass_ms(&self) -> Option<f32> {
        self.inner.borrow().pass_ns.map(ns_to_ms)
    }

    /// Per-category duration in milliseconds. `None` when
    /// `TIMESTAMP_QUERY_INSIDE_PASSES` is unavailable / disabled, or
    /// when the named category didn't run in the most recent measured
    /// frame.
    pub fn last_kind_ms(&self, kind: BatchKind) -> Option<f32> {
        self.inner.borrow().kind_ns[kind.idx()].map(ns_to_ms)
    }

    /// Pipeline-statistics counters around the main pass. `None` when
    /// `PIPELINE_STATISTICS_QUERY` is unavailable / disabled.
    pub fn last_pipeline_stats(&self) -> Option<PipelineStats> {
        self.inner.borrow().stats
    }

    pub(crate) fn record_pass_ns(&self, ns: u64) {
        self.inner.borrow_mut().pass_ns = Some(ns);
    }

    pub(crate) fn record_kind_ns(&self, kind: BatchKind, ns: u64) {
        self.inner.borrow_mut().kind_ns[kind.idx()] = Some(ns);
    }

    /// Clears every per-kind slot back to `None`. Called before
    /// publishing a fresh frame's per-kind values so categories that
    /// didn't run this frame don't keep showing the previous frame's
    /// number.
    pub(crate) fn clear_kinds(&self) {
        self.inner.borrow_mut().kind_ns = [None; <BatchKind as strum::EnumCount>::COUNT];
    }

    pub(crate) fn record_pipeline_stats(&self, raw: [u64; 5]) {
        self.inner.borrow_mut().stats = Some(PipelineStats {
            vertex_shader_invocations: raw[0],
            clipper_invocations: raw[1],
            clipper_primitives_out: raw[2],
            fragment_shader_invocations: raw[3],
            compute_shader_invocations: raw[4],
        });
    }
}

fn ns_to_ms(ns: u64) -> f32 {
    ns as f32 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_uninit() {
        let s = GpuPassStats::default();
        assert_eq!(s.last_pass_ms(), None);
        assert_eq!(s.last_kind_ms(BatchKind::Quads), None);
        assert_eq!(s.last_pipeline_stats(), None);
    }

    #[test]
    fn handle_clones_share_state() {
        let a = GpuPassStats::default();
        let b = a.clone();
        a.record_pass_ns(3_500_000);
        assert!((b.last_pass_ms().unwrap() - 3.5).abs() < 1e-4);
    }

    #[test]
    fn record_overrides_previous_value() {
        // Pin: stores the *latest* sample, not a rolling EMA.
        let s = GpuPassStats::default();
        s.record_pass_ns(1_000_000);
        s.record_pass_ns(5_000_000);
        assert!((s.last_pass_ms().unwrap() - 5.0).abs() < 1e-4);
    }

    #[test]
    fn per_kind_independent_of_total() {
        let s = GpuPassStats::default();
        s.record_kind_ns(BatchKind::Quads, 1_500_000);
        s.record_kind_ns(BatchKind::Text, 500_000);
        assert!((s.last_kind_ms(BatchKind::Quads).unwrap() - 1.5).abs() < 1e-4);
        assert!((s.last_kind_ms(BatchKind::Text).unwrap() - 0.5).abs() < 1e-4);
        assert_eq!(s.last_kind_ms(BatchKind::Mesh), None);
        // Total isn't auto-populated from per-kind.
        assert_eq!(s.last_pass_ms(), None);
    }

    #[test]
    fn clear_kinds_resets_to_none() {
        // Pin: a category that ran last frame but not this one shows
        // `None`, not the stale previous-frame value.
        let s = GpuPassStats::default();
        s.record_kind_ns(BatchKind::Quads, 2_000_000);
        s.clear_kinds();
        assert_eq!(s.last_kind_ms(BatchKind::Quads), None);
    }

    #[test]
    fn labels_match_lowercased_variant_names() {
        // Pin: `IntoStaticStr` + `serialize_all = "lowercase"` strips
        // the camel-case, no underscore. Adding a new variant breaks
        // this only if its name uses a multi-word form the lowercase
        // rule would mangle — choose names that round-trip cleanly.
        assert_eq!(BatchKind::Setup.label(), "setup");
        assert_eq!(BatchKind::PreClear.label(), "preclear");
        assert_eq!(BatchKind::Mask.label(), "mask");
        assert_eq!(BatchKind::Quads.label(), "quads");
        assert_eq!(BatchKind::Text.label(), "text");
        assert_eq!(BatchKind::Mesh.label(), "mesh");
        assert_eq!(BatchKind::Image.label(), "image");
        assert_eq!(BatchKind::Curve.label(), "curve");
    }

    #[test]
    fn pipeline_stats_round_trip() {
        let s = GpuPassStats::default();
        s.record_pipeline_stats([1, 2, 3, 4, 0]);
        let v = s.last_pipeline_stats().expect("recorded");
        assert_eq!(v.vertex_shader_invocations, 1);
        assert_eq!(v.clipper_invocations, 2);
        assert_eq!(v.clipper_primitives_out, 3);
        assert_eq!(v.fragment_shader_invocations, 4);
        assert_eq!(v.compute_shader_invocations, 0);
    }
}
