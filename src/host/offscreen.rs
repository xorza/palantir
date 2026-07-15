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

use crate::debug_overlay::DebugOverlayConfig;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::context::HostContext;
use crate::host::window_renderer::{PresentStrategy, WindowRenderer};
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::backend::{WgpuBackend, WgpuBackendConfig};
use crate::text::TextShaper;
use crate::ui::Ui;

/// One shared [`WgpuBackend`] + one [`WindowRenderer`], rendering to a
/// texture instead of a surface. The offscreen analogue of `WinitHost`.
#[derive(Debug)]
pub struct OffscreenHost {
    gpu: WgpuBackend,
    window: WindowRenderer,
}

/// Builder for [`OffscreenHost`] — see [`OffscreenHost::builder`]. The
/// required GPU/text resources come from that constructor; the rest start at
/// the general screenshot defaults.
#[derive(Debug)]
pub struct OffscreenHostBuilder {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shaper: TextShaper,
    collect_gpu_stats: bool,
    target_persists: bool,
    clock: Box<dyn Clock>,
}

impl OffscreenHostBuilder {
    /// Opt into GPU instrumentation (timestamp + pipeline-statistics
    /// queries). Default `false` — the per-frame readback is non-trivial.
    pub fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.collect_gpu_stats = collect;
        self
    }

    /// Declare that the caller reuses one render target across frames,
    /// enabling the direct-to-target / skip-noop fast paths. Default `false`:
    /// a fresh texture each `frame_offscreen` (screenshots, the visual
    /// harness) must be fully filled via backbuffer+copy.
    pub fn target_persists(mut self, persists: bool) -> Self {
        self.target_persists = persists;
        self
    }

    /// Per-frame time source. Default a wall-clock
    /// [`RealtimeClock`](crate::host::clock::RealtimeClock); a
    /// [`FixedClock`](crate::host::clock::FixedClock) makes the render reproducible
    /// (golden tests, thumbnails) — animations sample a fixed phase.
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    pub fn build(self) -> OffscreenHost {
        // The shared context outlives this call only as the clones in the
        // backend + window's `Ui`/`Frontend` (Rc-backed handles, including
        // the shared host state); the offscreen path never opens a second
        // window. The render target's format (per `frame_offscreen` call)
        // drives the lazy per-format pipeline build.
        let ctx = HostContext::new(self.shaper);
        let gpu = WgpuBackend::new(
            self.device,
            self.queue,
            &ctx,
            WgpuBackendConfig {
                collect_gpu_stats: self.collect_gpu_stats,
            },
        );
        // A reused target can take the direct-present path (full repaints go
        // straight in, small partials ride the backbuffer, skip frames keep its
        // last render); a fresh target each call must be fully filled via
        // backbuffer+copy.
        let strategy = if self.target_persists {
            PresentStrategy::DirectAdaptive
        } else {
            PresentStrategy::BackbufferCopy
        };
        let window = WindowRenderer::builder(&ctx, gpu.max_texture_dim())
            .strategy(strategy)
            .clock(self.clock)
            .build();
        OffscreenHost { gpu, window }
    }
}

impl OffscreenHost {
    /// Start building an offscreen host. `device` / `queue` / `shaper` are
    /// the GPU + text resources; the rest default to the general screenshot
    /// case (no GPU stats, a fresh target each frame, the wall clock) and are
    /// tuned via [`OffscreenHostBuilder`].
    pub fn builder(
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
    ) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            device,
            queue,
            shaper,
            collect_gpu_stats: false,
            target_persists: false,
            clock: Box::new(RealtimeClock::new()),
        }
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

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    use crate::host::clock::Clock;
    use crate::host::context::HostContext;
    use crate::host::window_renderer::{PresentStrategy, WindowRenderer};
    use crate::renderer::backend::{WgpuBackend, WgpuBackendConfig};
    use crate::text::TextShaper;
    use crate::ui::Ui;

    /// Two window render streams sharing one backend and host context. This is
    /// intentionally test-only: production multi-window ownership stays with
    /// `WinitHost`.
    #[derive(Debug)]
    pub struct TwoWindowOffscreenHost {
        gpu: WgpuBackend,
        windows: [WindowRenderer; 2],
    }

    impl TwoWindowOffscreenHost {
        pub fn new(
            device: wgpu::Device,
            queue: wgpu::Queue,
            shaper: TextShaper,
            clocks: [Box<dyn Clock>; 2],
        ) -> Self {
            let ctx = HostContext::new(shaper);
            let gpu = WgpuBackend::new(
                device,
                queue,
                &ctx,
                WgpuBackendConfig {
                    collect_gpu_stats: false,
                },
            );
            let max_texture_dim = gpu.max_texture_dim();
            let [clock_a, clock_b] = clocks;
            let window_a = WindowRenderer::builder(&ctx, max_texture_dim)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock_a)
                .build();
            let window_b = WindowRenderer::builder(&ctx, max_texture_dim)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock_b)
                .build();
            Self {
                gpu,
                windows: [window_a, window_b],
            }
        }

        pub fn frame_offscreen(
            &mut self,
            window: usize,
            target: &wgpu::Texture,
            scale_factor: f32,
            record: impl FnMut(&mut Ui),
        ) {
            self.windows[window].frame_offscreen(&mut self.gpu, target, scale_factor, record);
        }
    }
}
