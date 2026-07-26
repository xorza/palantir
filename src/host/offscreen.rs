//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Both build on the same [`HostCore`]: one
//! [`HostShared`](crate::host::shared::HostShared), one
//! [`Frontend`](crate::renderer::frontend::Frontend), one
//! [`WgpuBackend`](crate::renderer::backend::WgpuBackend), and one
//! [`WindowDriver`] per render stream. Unlike `WinitHost` there's no winit and
//! no swapchain — each stream renders into a caller-supplied `wgpu::Texture`.
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
//! **One stream, and no window lifecycle.** The host drives exactly the window
//! its builder names, for as long as it lives. A frame that records
//! [`Ui::open_window`] or [`Ui::close_window`] **panics** rather than silently
//! discarding the request, since nothing here can service one and a swallowed
//! request leaves the app believing a window appeared. Multi-window ownership
//! is `WinitHost`'s job; the `internals`-gated `TwoWindowOffscreenHost` exists
//! only so the visual suite can pin two drivers sharing one core.

use crate::FrameReport;
use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::gpu_stats::GpuPassStats;
use crate::display::{self, Display};
use crate::host::clock::{Clock, RealtimeClock};
use crate::host::core::HostCore;
use crate::host::window_driver::{CpuFrame, PresentStrategy, TargetKey, WindowDriver};
use crate::primitives::approx::EPS;
use crate::renderer::backend::BackendConfig;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::window::WindowToken;

/// An offscreen frame was given a non-finite or near-zero display scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvalidScaleFactorError {
    /// The rejected logical-to-physical conversion factor.
    pub scale_factor: f32,
}

impl std::fmt::Display for InvalidScaleFactorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "offscreen scale factor must be finite and at least {EPS}, got {}",
            self.scale_factor
        )
    }
}

impl std::error::Error for InvalidScaleFactorError {}

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
    token: WindowToken,
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

    /// Allocate the shared core and the first window driver from the sealed
    /// settings.
    pub fn build(self) -> OffscreenHost {
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
            .driver(self.token)
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
    /// Start building an offscreen host whose single window is addressed by
    /// `token`. The text shaper defaults to bundled fonts, GPU timing
    /// defaults off, the clock defaults to realtime, and physical-pixel
    /// snapping defaults on.
    pub fn builder(
        token: WindowToken,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            token,
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
    /// call. The target may be replaced between calls. The host's
    /// [`WindowToken`] is passed to [`App::update`] and [`App::record`], with
    /// the same once-only update and replayable record semantics as
    /// [`crate::WinitHost`].
    ///
    /// # Errors
    ///
    /// Returns [`InvalidScaleFactorError`] before changing host or application
    /// state when `scale_factor` is non-finite or less than `1e-4`.
    ///
    /// # Panics
    ///
    /// Panics if the frame recorded [`Ui::open_window`] or
    /// [`Ui::close_window`] — this host has no window lifecycle.
    pub fn frame_offscreen<T: App>(
        &mut self,
        target: &wgpu::Texture,
        scale_factor: f32,
        app: &mut T,
    ) -> Result<FrameReport, InvalidScaleFactorError> {
        render_frame(&mut self.core, &mut self.driver, target, scale_factor, app)
    }

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.core.shared.resources.diagnostics.gpu_pass_stats
    }
}

/// The offscreen frame, free-standing so the `internals` two-window harness
/// can drive two drivers through one core without duplicating it.
fn render_frame<T: App>(
    core: &mut HostCore,
    driver: &mut WindowDriver,
    target: &wgpu::Texture,
    scale_factor: f32,
    app: &mut T,
) -> Result<FrameReport, InvalidScaleFactorError> {
    validate_scale_factor(scale_factor)?;

    let key = TargetKey::of(target);
    driver.note_target(key);
    let display = Display {
        pixel_snap: driver.pixel_snap,
        ..Display::from_physical(key.physical, scale_factor)
    };
    let CpuFrame { report, mode } = core.cpu_frame(driver, display, app);
    // Before submitting: a frame that asked for a window it can never get is a
    // caller error, and reporting it against an untouched target keeps the
    // failure clean.
    driver.deny_window_requests();
    core.submit(driver, target, mode);
    Ok(report)
}

fn validate_scale_factor(scale_factor: f32) -> Result<(), InvalidScaleFactorError> {
    if display::scale_factor_is_valid(scale_factor) {
        Ok(())
    } else {
        Err(InvalidScaleFactorError { scale_factor })
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::app::internals::RecordApp;
    use crate::common::clipboard::Clipboard;
    use crate::host::clock::Clock;
    use crate::host::core::HostCore;
    use crate::host::offscreen::{self, InvalidScaleFactorError, OffscreenHost};
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::renderer::backend::BackendConfig;
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::text::TextShaper;
    use crate::ui::Ui;
    use crate::window::WindowToken;

    /// Draw list the most recent [`OffscreenHost::frame_offscreen`]
    /// composed. The `record_pass` benchmark replays the schedule over it
    /// to report the exact step counts behind each timing — a number the
    /// backend never publishes, because counting steps on the production
    /// path would cost what the benchmark exists to measure.
    pub(crate) fn last_render_buffer(host: &OffscreenHost) -> &RenderBuffer {
        &host.core.frontend.buffer
    }

    /// Two render streams sharing one [`HostCore`] — the headless stand-in for
    /// two winit windows, used by the visual suite to pin that interleaved
    /// windows keep their own retained pixels and owner-scoped `GpuView`
    /// targets.
    ///
    /// Test-only on purpose: [`OffscreenHost`] drives exactly one stream, and
    /// production multi-window ownership belongs to
    /// [`WinitHost`](crate::WinitHost). This exists solely because proving
    /// backend sharing needs two drivers on *one* core, which two separate
    /// hosts cannot express.
    #[derive(Debug)]
    pub struct TwoWindowOffscreenHost {
        core: HostCore,
        windows: [WindowDriver; 2],
    }

    impl TwoWindowOffscreenHost {
        pub fn new(
            device: wgpu::Device,
            queue: wgpu::Queue,
            shaper: TextShaper,
            clocks: [Box<dyn Clock>; 2],
        ) -> Self {
            let core = HostCore::new(
                device,
                queue,
                shaper,
                Clipboard::default(),
                BackendConfig::default(),
            );
            let [clock_a, clock_b] = clocks;
            // Token equals the array index the harness addresses.
            let windows = [
                Self::stream(&core, WindowToken(0), clock_a),
                Self::stream(&core, WindowToken(1), clock_b),
            ];
            Self { core, windows }
        }

        fn stream(core: &HostCore, token: WindowToken, clock: Box<dyn Clock>) -> WindowDriver {
            core.driver(token)
                .strategy(PresentStrategy::BackbufferCopy)
                .clock(clock)
                .build()
        }

        pub fn frame_offscreen(
            &mut self,
            window: usize,
            target: &wgpu::Texture,
            scale_factor: f32,
            record: impl FnMut(&mut Ui),
        ) -> Result<(), InvalidScaleFactorError> {
            let mut app = RecordApp::new(record);
            offscreen::render_frame(
                &mut self.core,
                &mut self.windows[window],
                target,
                scale_factor,
                &mut app,
            )
            .map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::host::offscreen::{InvalidScaleFactorError, validate_scale_factor};
    use crate::primitives::approx::EPS;

    #[test]
    fn scale_validation_rejects_invalid_values_and_accepts_boundary() {
        for scale_factor in [
            0.0,
            -1.0,
            EPS / 2.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
        ] {
            let error = validate_scale_factor(scale_factor).unwrap_err();
            assert_eq!(error.scale_factor.to_bits(), scale_factor.to_bits());
        }

        assert_eq!(validate_scale_factor(EPS), Ok(()));
        assert_eq!(validate_scale_factor(1.0), Ok(()));
        assert_eq!(
            validate_scale_factor(0.0),
            Err(InvalidScaleFactorError { scale_factor: 0.0 })
        );
        assert_eq!(
            InvalidScaleFactorError { scale_factor: 0.0 }.to_string(),
            "offscreen scale factor must be finite and at least 0.0001, got 0"
        );
    }
}
