//! [`RenderContext`] — the cross-window, GPU-agnostic shared resources
//! that both the recorder ([`Ui`]) and the GPU renderer
//! ([`WgpuBackend`](crate::renderer::backend::WgpuBackend)) are built
//! from: the text shaper, the per-frame arena, the CPU-side render
//! caches, and the GPU-stats handle.
//!
//! These are owned here — *not* on the backend — so constructing a `Ui`
//! depends only on the shared UI/render resources, never on the GPU
//! renderer. The host builds one `RenderContext`, hands it to
//! `WgpuBackend::new` (which clones what it needs), and makes every
//! window's `Ui` + `Frontend` from it. All four handles are cheap
//! Rc/Arc-backed clones, so the backend's and each `Ui`'s copies all point
//! at one shared set.

use crate::forest::frame_arena::FrameArena;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::caches::RenderCaches;
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;
use crate::ui::Ui;

/// Shared, GPU-agnostic resources cloned into every window's `Ui` +
/// `Frontend` and into the one shared backend. One per app.
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
            frame_arena: FrameArena::default(),
            caches: RenderCaches::default(),
            pass_stats: GpuPassStats::default(),
        }
    }

    /// A fresh per-window [`Ui`] sharing this context's shaper, frame
    /// arena, render caches, and GPU-stats handle. No GPU backend needed.
    pub(crate) fn make_ui(&self) -> Ui {
        Ui::new(
            self.shaper.clone(),
            self.frame_arena.clone(),
            self.caches.clone(),
            self.pass_stats.clone(),
        )
    }

    /// A fresh per-window [`Frontend`] (CPU encode/compose scratch)
    /// sharing this context's frame arena.
    pub(crate) fn make_frontend(&self) -> Frontend {
        Frontend::new(self.frame_arena.clone())
    }
}
