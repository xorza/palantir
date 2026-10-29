//! One frame of work handed to the backend.

use crate::damage::Damage;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::repaint_scissors::RepaintScissors;
use crate::shape::record_store::RecordStore;

/// What [`WgpuBackend::submit`](crate::gpu::wgpu_backend::WgpuBackend::submit) draws: the
/// frame's draw list and record store, the scissors it repaints inside, and how it reaches the
/// target.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Submission<'a> {
    /// Where the owner's GPU timings publish.
    pub(crate) stats: &'a GpuPassStats,
    pub(crate) store: &'a RecordStore,
    pub(crate) buffer: &'a RenderBuffer,
    /// The plan's clear color, which a root quad folded into the clear overrides.
    pub(crate) clear: RgbaF32,
    /// What the frame repaints inside, which the CPU half fitted to the draw list: a partial
    /// frame loads the target and repaints the rects.
    pub(crate) scissors: &'a RepaintScissors,
    pub(crate) route: Route,
    pub(crate) overlay: FrameOverlay,
}

/// How a painted frame reaches its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// Painted into the target itself.
    Direct,
    /// Painted into the window's backbuffer, then copied onto the target.
    ViaBackbuffer,
}

/// The debug overlays a frame draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameOverlay {
    /// Dim what a partial frame does not repaint.
    pub(crate) dim_undamaged: bool,
    /// Outline this damage on the target, where the damage-rect overlay is on.
    pub(crate) damage: Option<Damage>,
}

impl Submission<'_> {
    /// The opaque clear the frame paints: the buffer's override, else the plan's.
    pub(crate) fn clear(&self) -> RgbaF16 {
        let clear = self.buffer.clear_override.map_or(self.clear, RgbaF32::from);
        RgbaF32 { a: 1.0, ..clear }.to_f16()
    }

    /// Whether the frame dims what it does not repaint: only a partial frame keeps any.
    pub(crate) const fn dims_undamaged(&self) -> bool {
        self.overlay.dim_undamaged && self.scissors.is_partial()
    }
}
