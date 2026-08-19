//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Both build on the same [`HostCore`]: one
//! [`HostShared`](crate::host::shared::HostShared), one
//! [`Frontend`](crate::renderer::frontend::Frontend), one
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend), and one
//! [`WindowDriver`]. Unlike `WinitHost` there's no winit and no swapchain —
//! the driver renders into a caller-supplied `wgpu::Texture`.
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
//!
//! **One window, and no window lifecycle.** The window is created with the
//! host and addressed by the fixed [`OffscreenHost::WINDOW`] for as long as
//! the host lives — there is no window API at all. A frame that records
//! [`Ui::open_window`] or [`Ui::close_window`] **panics** rather than silently
//! discarding the request, since nothing here can service one and a swallowed
//! request leaves the app believing a window appeared. Multi-window ownership
//! is `WinitHost`'s alone.
//!
//! Everything the host can reject is a caller mistake — an unusable scale
//! factor, a window request it has no lifecycle for — so `frame_offscreen`
//! panics rather than returning a `Result` no caller could act on.

use crate::FrameReport;
use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::gpu_stats::GpuPassStats;
use crate::display;
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::core::HostCore;
use crate::host::device_requirements::DeviceRequirements;
use crate::host::window_driver::{CpuFrame, PresentStrategy, TargetKey, WindowDriver};
use crate::primitives::approx::EPS;
use crate::renderer::backend::BackendConfig;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::window::WindowToken;

/// One shared renderer driving one render stream into a texture instead of a
/// surface. The offscreen analogue of `WinitHost`.
#[derive(Debug)]
pub struct OffscreenHost {
    core: HostCore,
    driver: WindowDriver,
}

/// Seals offscreen policy before allocating the backend and window driver.
#[derive(Debug)]
pub struct OffscreenHostBuilder {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// `None` until [`Self::shaper`] overrides; resolved to the
    /// bundled-fonts default lazily in [`Self::build`] so an override
    /// never pays the font load.
    shaper: Option<TextShaper>,
    collect_gpu_stats: bool,
    clock: Box<dyn Clock>,
    pixel_snap: bool,
}

impl OffscreenHostBuilder {
    /// Replace the default bundled-fonts [`TextShaper`], so several hosts
    /// can share one shaped-buffer cache.
    ///
    /// Real shaping either way: `TextShaper`'s only production constructors
    /// load the bundled fonts, and the font-free mono metric is reachable
    /// solely through `test_mono`, which the `internals` feature gates out
    /// of production builds. A released binary therefore cannot drive an
    /// offscreen host on placeholder metrics through this builder.
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.shaper = Some(shaper);
        self
    }

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

    /// Allocate the shared core and the window driver from the sealed
    /// settings.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot run Palantir's pipelines — see
    /// [`DeviceRequirements`]. Checked here rather than left to the first
    /// pipeline that trips over it, because a device is only ever short of a
    /// feature its own request forgot to ask for: by the time one exists,
    /// `request_device` has already granted whatever was asked. The mistake is
    /// upstream of this call, so the report belongs at this boundary and not
    /// several layers into the backend.
    pub fn build(self) -> OffscreenHost {
        if let Err(unmet) = DeviceRequirements::met_by(&self.device) {
            panic!("offscreen host device cannot run Palantir: {unmet}");
        }
        let core = HostCore::new(
            self.device,
            self.queue,
            self.shaper.unwrap_or_default(),
            Clipboard::default(),
            BackendConfig {
                collect_gpu_stats: self.collect_gpu_stats,
            },
        );
        let driver = core
            .driver(OffscreenHost::WINDOW)
            // The target's prior contents can't be relied on (a caller may
            // hand in a fresh texture each call), so every frame must fill the
            // whole thing.
            .strategy(PresentStrategy::BackbufferCopy)
            .clock(self.clock)
            .pixel_snap(self.pixel_snap)
            .build();
        OffscreenHost { core, driver }
    }
}

impl OffscreenHost {
    /// The token this host's one window is addressed by — handed to
    /// [`App::update`] and [`App::record`], and all an offscreen app ever
    /// sees. Fixed rather than caller-chosen: there is exactly one window and
    /// no lifecycle, so a choice here would carry no information.
    pub const WINDOW: WindowToken = WindowToken(0);

    /// Start building an offscreen host. The text shaper defaults to bundled
    /// fonts, GPU timing defaults off, the clock defaults to realtime, and
    /// physical-pixel snapping defaults on.
    pub fn builder(device: wgpu::Device, queue: wgpu::Queue) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            device,
            queue,
            shaper: None,
            collect_gpu_stats: false,
            clock: Box::new(RealtimeClock::new()),
            pixel_snap: true,
        }
    }

    /// Mutable access to the window's `Ui` for building scenes.
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    /// Set the app-global debug overlay for subsequent frames. The
    /// headless analogue of a `WinitHost` window toggling it via
    /// `Ui::debug_overlay_mut` — it writes the same shared diagnostics state
    /// every window's `Ui` reads.
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        *self.core.shared.resources.diagnostics.overlay.borrow_mut() = overlay;
    }

    /// Run one offscreen application frame against `target`, filling the
    /// supplied texture even when the UI has not changed since the previous
    /// call. The target may be replaced between calls. [`Self::WINDOW`] is
    /// passed to [`App::update`] and [`App::record`], with the same once-only
    /// update and replayable record semantics as [`crate::WinitHost`].
    ///
    /// # Panics
    ///
    /// Panics if `scale_factor` is non-finite or below `1e-4`, or if the frame
    /// recorded [`Ui::open_window`] / [`Ui::close_window`] — this host has no
    /// window lifecycle.
    pub fn frame_offscreen<T: App>(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        app: &mut T,
    ) -> FrameReport {
        assert!(
            display::scale_factor_is_valid(scale_factor),
            "offscreen scale factor must be finite and at least {EPS}, got \
             {scale_factor}"
        );

        let key = TargetKey::of(target);
        let driver = &mut self.driver;
        driver.note_target(key);
        // No monitor, so no refresh rate to declare.
        let display = driver.display(key.physical, scale_factor, None);
        let CpuFrame { report, mode } = self.core.cpu_frame(driver, display, app);
        // Before submitting: a frame that asked for a window it can never get
        // is a caller error, and reporting it against an untouched target
        // keeps the failure clean.
        driver.deny_window_requests();
        self.core.submit(driver, target, mode);
        report
    }

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.core.shared.resources.diagnostics.gpu_pass_stats
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
        self.core.backend.has_format_pipelines(format)
    }

    /// Images resident in the GPU texture cache. Used by the format-change
    /// test to assert the cache survives a new format's pipeline build (no
    /// re-upload).
    pub fn gpu_image_cache_len(&self) -> usize {
        self.core.backend.gpu_image_cache_len()
    }
}

#[cfg(feature = "bench")]
pub(crate) mod internals {
    use crate::host::offscreen::OffscreenHost;
    use crate::renderer::render_buffer::RenderBuffer;

    /// Draw list the most recent [`OffscreenHost::frame_offscreen`]
    /// composed. The `record_pass` benchmark replays the schedule over it
    /// to report the exact step counts behind each timing — a number the
    /// backend never publishes, because counting steps on the production
    /// path would cost what the benchmark exists to measure.
    pub(crate) fn last_render_buffer(host: &OffscreenHost) -> &RenderBuffer {
        &host.core.frontend.buffer
    }
}
