//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Like `WinitHost` it owns the one
//! shared [`WgpuBackend`] and drives a [`WindowRenderer`] (built from a
//! [`HostContext`]); unlike it there's no winit, no swapchain, and exactly
//! one window — it renders to a caller-supplied `wgpu::Texture`.
//! [`OffscreenHost::frame_offscreen`] accepts the same [`App`] lifecycle as
//! the windowed host, so update and replay semantics do not depend on the
//! output backend.
//!
//! A supported headless rendering entry point — render-to-texture for
//! screenshots, thumbnails, or server-side compositing — that also backs
//! the visual harness and GPU benches. It's a `pub` facade because
//! `WgpuBackend` is `pub(crate)` and can't be named from an external crate,
//! so callers drive the backend through this bundle. The two
//! cache-introspection methods stay `internals`-gated: they call gated
//! `WgpuBackend` helpers and exist only for the format-change test.

use crate::app::App;
use crate::debug_overlay::DebugOverlayConfig;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::context::HostContext;
use crate::host::window_renderer::{PresentStrategy, WindowRenderer};
use crate::renderer::backend::WgpuBackend;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::WindowToken;

/// One shared `WgpuBackend` + one `WindowRenderer`, rendering to a
/// texture instead of a surface. The offscreen analogue of `WinitHost`.
#[derive(Debug)]
pub struct OffscreenHost {
    gpu: WgpuBackend,
    window: WindowRenderer,
}

/// Seals offscreen policy before allocating the backend and window renderer.
#[derive(Debug)]
pub struct OffscreenHostBuilder {
    token: WindowToken,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shaper: TextShaper,
    collect_gpu_stats: bool,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
}

impl OffscreenHostBuilder {
    /// Opt into GPU timestamp and pipeline-statistics collection. The supplied
    /// device must have the corresponding wgpu features enabled.
    pub fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.collect_gpu_stats = collect;
        self
    }

    /// Replace the realtime clock. A [`FixedClock`](crate::FixedClock) makes
    /// screenshots and thumbnails reproducible by holding animations at a
    /// caller-controlled phase.
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Configure whether axis-aligned paint edges snap to physical pixels.
    pub fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    /// Allocate the backend and window renderer from the sealed settings.
    pub fn build(self) -> OffscreenHost {
        let ctx = HostContext::new(self.shaper);
        ctx.set_open_windows([self.token]);
        let gpu = WgpuBackend::new(self.device, self.queue, &ctx)
            .collect_gpu_stats(self.collect_gpu_stats, ctx.pass_stats.clone());
        let window = WindowRenderer::builder(self.token, &ctx, gpu.max_texture_dim())
            .strategy(PresentStrategy::BackbufferCopy)
            .clock(self.clock)
            .pixel_snap(self.pixel_snap)
            .build();
        OffscreenHost { gpu, window }
    }
}

impl OffscreenHost {
    /// Start building an offscreen host whose single window is addressed by
    /// `token`. GPU timing defaults off, the clock defaults to realtime, and
    /// physical-pixel snapping defaults on.
    pub fn builder(
        token: WindowToken,
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
    ) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            token,
            device,
            queue,
            shaper,
            collect_gpu_stats: false,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap: true,
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

    /// Run one offscreen application frame against `target`, filling the
    /// supplied texture even when the UI has not changed since the previous
    /// call. The target may be replaced between calls. The host's
    /// [`WindowToken`] is passed to [`App::update`] and [`App::record`], with
    /// the same once-only update and replayable record semantics as
    /// [`crate::WinitHost`].
    pub fn frame_offscreen<T: App>(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        app: &mut T,
    ) {
        self.window
            .frame_offscreen(&mut self.gpu, target, scale_factor, app);
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

#[cfg(feature = "internals")]
pub(crate) mod test_support {
    use crate::app::test_support::RecordApp;
    use crate::host::clock::Clock;
    use crate::host::context::HostContext;
    use crate::host::window_renderer::{PresentStrategy, WindowRenderer};
    use crate::renderer::backend::WgpuBackend;
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::window::WindowToken;

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
            ctx.set_open_windows([WindowToken(0), WindowToken(1)]);
            let gpu = WgpuBackend::new(device, queue, &ctx);
            let max_texture_dim = gpu.max_texture_dim();
            let [clock_a, clock_b] = clocks;
            let window_a = WindowRenderer::builder(WindowToken(0), &ctx, max_texture_dim)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock_a)
                .build();
            let window_b = WindowRenderer::builder(WindowToken(1), &ctx, max_texture_dim)
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
            let mut app = RecordApp::new(record);
            self.windows[window].frame_offscreen(&mut self.gpu, target, scale_factor, &mut app);
        }
    }
}
