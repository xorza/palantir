//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Like `WinitHost` it owns the one
//! shared [`WgpuBackend`] and drives a [`WindowRenderer`] (built from a
//! [`HostContext`]); unlike it there's no winit, no swapchain, and exactly
//! one window — it renders to a caller-supplied `wgpu::Texture`.
//!
//! A supported headless rendering entry point — render-to-texture for
//! screenshots, thumbnails, or server-side compositing — that also backs
//! the visual harness and GPU benches. It's a `pub` facade because
//! `WgpuBackend` is `pub(crate)` and can't be named from an external crate,
//! so callers drive the backend through this bundle. The two
//! cache-introspection methods stay `internals`-gated: they call gated
//! `WgpuBackend` helpers and exist only for the format-change test.

use crate::context::HostContext;
use crate::debug_overlay::DebugOverlayConfig;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::backend::{WgpuBackend, WgpuBackendConfig};
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window_renderer::{PresentStrategy, WindowRenderer};

/// One shared [`WgpuBackend`] + one [`WindowRenderer`], rendering to a
/// texture instead of a surface. The offscreen analogue of `WinitHost`.
pub struct OffscreenHost {
    gpu: WgpuBackend,
    window: WindowRenderer,
}

impl OffscreenHost {
    /// `target_persists` declares whether the caller reuses one render target
    /// across frames (enabling the direct-to-target / skip-noop fast paths) or
    /// supplies a fresh texture each `frame_offscreen` call (screenshots, the
    /// visual harness — every frame must fill the whole target via
    /// backbuffer+copy).
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
        collect_gpu_stats: bool,
        target_persists: bool,
    ) -> Self {
        // The shared context outlives this call only as the clones in the
        // backend + window's `Ui`/`Frontend` (Rc-backed handles, including
        // the shared host state); the offscreen path never opens a second
        // window. The render target's format (per `frame_offscreen` call)
        // drives the lazy per-format pipeline build.
        let ctx = HostContext::new(shaper);
        let gpu = WgpuBackend::new(device, queue, &ctx, WgpuBackendConfig { collect_gpu_stats });
        // A reused target can take the direct-present path (full repaints go
        // straight in, small partials ride the backbuffer, skip frames keep its
        // last render); a fresh target each call must be fully filled via
        // backbuffer+copy.
        let strategy = if target_persists {
            PresentStrategy::DirectAdaptive
        } else {
            PresentStrategy::BackbufferCopy
        };
        let window = WindowRenderer::new(&ctx, gpu.max_texture_dim(), strategy);
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

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.window.ui.ctx.pass_stats
    }
}

/// Cache-introspection peepholes for the visual format-change test. Gated
/// because they call `internals`-gated `WgpuBackend` helpers.
#[cfg(any(test, feature = "internals"))]
impl OffscreenHost {
    /// Whether the shared backend has built a pipeline set for `format`.
    /// Lets format-change tests confirm a new format materializes its own
    /// pipelines.
    pub fn has_format_pipelines(&self, format: wgpu::TextureFormat) -> bool {
        self.gpu.has_format_pipelines(format)
    }

    /// Images resident in the GPU texture cache. Used by the format-change
    /// test to assert the cache survives a new format's pipeline build (no
    /// re-upload).
    pub fn gpu_image_cache_len(&self) -> usize {
        self.gpu.gpu_image_cache_len()
    }
}
