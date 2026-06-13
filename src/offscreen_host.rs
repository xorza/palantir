//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Like `WinitHost` it owns the one
//! shared [`WgpuBackend`] + [`HostContext`] and drives a
//! [`WindowRenderer`]; unlike it there's no winit, no swapchain, and
//! exactly one window — it renders to a caller-supplied `wgpu::Texture`.
//!
//! Test/bench-only (gated behind `internals`): the visual harness and the
//! GPU benches use it because `WgpuBackend` is `pub(crate)` and can't be
//! named from an external crate, so they need this `pub` facade bundling
//! the backend with its window.

use crate::context::HostContext;
use crate::debug_overlay::DebugOverlayConfig;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::backend::{WgpuBackend, WgpuBackendConfig};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window_renderer::WindowRenderer;

/// One shared [`WgpuBackend`] + one [`WindowRenderer`], rendering to a
/// texture instead of a surface. The offscreen analogue of `WinitHost`.
pub struct OffscreenHost {
    gpu: WgpuBackend,
    window: WindowRenderer,
}

impl OffscreenHost {
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
        collect_gpu_stats: bool,
    ) -> Self {
        // The shared context outlives this call only as the clones in the
        // backend + window's `Ui`/`Frontend` (Rc-backed handles, including
        // the shared host state); the offscreen path never opens a second
        // window. The render target's format (per `frame_offscreen` call)
        // drives the lazy per-format pipeline build.
        let ctx = HostContext::new(shaper);
        let gpu = WgpuBackend::new(device, queue, &ctx, WgpuBackendConfig { collect_gpu_stats });
        let window = WindowRenderer::new(&ctx);
        Self { gpu, window }
    }

    /// Mutable access to the window's `Ui` for building scenes.
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.window.ui
    }

    /// Set the app-global debug overlay for subsequent frames. The
    /// headless analogue of a `WinitHost` window toggling it via
    /// `Ui::debug_overlay_mut` — it writes the same shared context state the
    /// window's `Ui` reads.
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        *self.window.ui.ctx.debug_overlay_mut() = overlay;
    }

    /// Run one offscreen frame against `target`.
    pub fn frame_offscreen(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        record: impl FnMut(&mut Ui),
    ) {
        self.window
            .frame_offscreen(&mut self.gpu, target, scale_factor, record);
    }

    /// Whether the shared backend has built a pipeline set for `format`.
    /// Lets format-change tests confirm a new format materializes its own
    /// pipelines.
    pub fn has_format_pipelines(&self, format: wgpu::TextureFormat) -> bool {
        self.gpu.has_format_pipelines(format)
    }

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.window.ui.ctx.pass_stats
    }

    /// Images resident in the GPU texture cache. Used by the format-change
    /// test to assert the cache survives a new format's pipeline build (no
    /// re-upload).
    pub fn gpu_image_cache_len(&self) -> usize {
        self.gpu.gpu_image_cache_len()
    }
}
