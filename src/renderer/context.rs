//! [`RenderContext`] — the cross-window, GPU-agnostic shared resources
//! that both the recorder ([`Ui`]) and the GPU renderer
//! ([`WgpuBackend`](crate::renderer::backend::WgpuBackend)) are built
//! from: the text shaper, the per-frame arena, the CPU-side render
//! caches, and the GPU-stats handle.
//!
//! These are owned here — *not* on the backend — so constructing a `Ui`
//! depends only on the shared UI/render resources, never on the GPU
//! renderer. It's a passive resource bag, not a factory: the host builds
//! one `RenderContext`, hands it to `WgpuBackend::new` (which clones what
//! it needs) and to `Ui::new` / `Frontend::new` (which pull the handles
//! they need). All the handles are cheap Rc/Arc-backed clones, so the
//! backend's and each `Ui`'s copies all point at one shared set.

use crate::forest::frame_arena::FrameArena;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::caches::RenderCaches;
use crate::text::TextShaper;

/// Shared, GPU-agnostic resources cloned into every window's `Ui` +
/// `Frontend` and into the one shared backend. One per app. Cloning is
/// cheap — every field is an Rc/Arc-backed handle pointing at one set.
#[derive(Clone, Default)]
pub(crate) struct RenderContext {
    pub(crate) shaper: TextShaper,
    pub(crate) frame_arena: FrameArena,
    pub(crate) caches: RenderCaches,
    pub(crate) pass_stats: GpuPassStats,
}

impl RenderContext {
    /// Build the shared context around `shaper` (the caller supplies it so
    /// headless harnesses can share a process/thread-local shaper). The
    /// frame arena, render caches, and GPU-stats handle are fresh.
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            shaper,
            ..Default::default()
        }
    }
}
