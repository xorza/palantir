//! [`OffscreenHost`] — the headless peer of
//! [`WinitHost`](crate::WinitHost). Both build on the same [`HostCore`]: one
//! [`UiResources`](crate::ui::resources::UiResources), one
//! [`Frontend`](crate::renderer::frontend::Frontend), one
//! [`WgpuBackend`](crate::gpu::wgpu_backend::WgpuBackend), and one
//! [`WindowDriver`]. Unlike `WinitHost` there's no winit and no swapchain —
//! the driver renders into a caller-supplied [`RenderTarget`].
//! [`OffscreenHost::frame`] accepts the same [`App`] lifecycle as
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
//! **Window *settings* are accepted and inert**, which is the other half of
//! the same rule: [`Ui::set_cursor`](crate::Ui::set_cursor) and
//! [`Ui::set_vsync`](crate::Ui::set_vsync) are levels the recorder retains
//! and reads back — `Ui::vsync` answers what the app set, headless or not —
//! so there is nothing for this host to swallow. Opens and closes are edges
//! that mean nothing unless serviced; settings are not. That is the whole of
//! which window calls this host honours.
//!
//! Everything the host can reject is a caller mistake — an unusable system
//! scale, a window request it has no lifecycle for — so `frame`
//! panics rather than returning a `Result` no caller could act on.

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::display;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::host::clock::Clock;
use crate::host::core::{HostCore, HostCoreConfig};
use crate::host::window_driver::{CpuFrame, PresentStrategy, TargetKey, WindowDriver};
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::primitives::math::domain::EPS;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::window::window_token::WindowToken;

/// One shared renderer driving one render stream into a texture instead of a
/// surface. The offscreen analogue of `WinitHost`.
#[derive(Debug)]
pub struct OffscreenHost {
    core: HostCore,
    driver: WindowDriver,
}

/// Seals offscreen policy before allocating the backend and window driver.
#[derive(Debug)]
#[must_use]
pub struct OffscreenHostBuilder {
    gpu: Gpu,
    /// See [`Self::retained_target`].
    retained_target: bool,
    /// `None` until [`Self::fonts`] or [`Self::shaper`] overrides; resolved
    /// to the bundled-fonts default lazily in [`Self::build`] so an override
    /// never pays the font load.
    shaper: Option<TextShaper>,
    collect_gpu_stats: bool,
    /// `None` leaves the driver's own default standing — see
    /// [`WindowDriver::builder`](crate::host::window_driver::WindowDriver).
    /// Held rather than applied, because the driver builder wants a
    /// `UiResources` that does not exist until [`Self::build`]; restating the
    /// defaults here instead is what let the two drift.
    clock: Option<Box<dyn Clock>>,
    pixel_snap: bool,
    /// Off by default. An offscreen host is as often a thumbnailer or a
    /// server-side compositor as it is an application, and neither should
    /// reach for the desktop's clipboard because it happened to record a
    /// `TextEdit`.
    #[cfg(feature = "system-clipboard")]
    system_clipboard: bool,
}

impl OffscreenHostBuilder {
    /// Which faces this host shapes against. Defaults to
    /// [`FontScope::Bundled`], so an offscreen render measures the same on
    /// every machine.
    ///
    /// The door both hosts share — see
    /// [`WinitHostBuilder::fonts`](crate::WinitHostBuilder::fonts), which
    /// defaults the other way. Builds a fresh [`TextShaper`] and so
    /// replaces one set by [`Self::shaper`].
    pub fn fonts(mut self, scope: FontScope) -> Self {
        self.shaper = Some(TextShaper::with_fonts(scope));
        self
    }

    /// Replace the default bundled-fonts [`TextShaper`], so several hosts
    /// can share one shaped-buffer cache.
    ///
    /// The headless escape hatch past [`Self::fonts`]: a caller that
    /// already built a shaper — with its warmed cache, or with fonts
    /// loaded into it — hands that one over instead of a scope to build a
    /// second from.
    ///
    /// Real shaping either way: every `TextShaper` carries a font
    /// database, and the placeholder *metric* is reachable solely through
    /// `test_mono`, which the `internals` feature gates out of production
    /// builds. A released binary therefore cannot drive an offscreen host
    /// on placeholder measurements through this builder.
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.shaper = Some(shaper);
        self
    }

    /// Opt into GPU timestamp and pipeline-statistics collection. The supplied
    /// device must have the corresponding wgpu features enabled.
    pub const fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.collect_gpu_stats = collect;
        self
    }

    /// Replace the realtime clock. A [`FixedClock`](crate::FixedClock) makes
    /// screenshots and thumbnails reproducible by holding animations at a
    /// caller-controlled phase.
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Some(Box::new(clock));
        self
    }

    /// Promise that every frame renders into the *same* texture, so the
    /// target keeps what the last frame drew.
    ///
    /// Off by default, because the safe assumption about a caller-supplied
    /// texture is that it is a fresh one: without last frame's pixels, a
    /// partial repaint would leave the rest of the target undefined, so every
    /// frame goes through the retained backbuffer and is copied out whole.
    ///
    /// A caller that loops on one texture — a thumbnailer, a server-side
    /// compositor — pays that backbuffer and that copy for nothing. Saying so
    /// here renders straight into the target instead, and lets small damage
    /// repaint only the damage.
    ///
    /// **Only say it if it is true.** Turning this on while handing in a fresh
    /// texture each call leaves everything outside the damage region holding
    /// whatever that texture happened to contain.
    pub const fn retained_target(mut self, retained: bool) -> Self {
        self.retained_target = retained;
        self
    }

    /// Configure whether axis-aligned paint edges snap to physical pixels.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    /// Back this host's [`Clipboard`] with the OS
    /// clipboard instead of the in-process buffer.
    ///
    /// Off by default, and deliberately: a thumbnailer or a server-side
    /// compositor renders text fields it never intends to let the user
    /// cut from, and reaching for the desktop's clipboard on their behalf
    /// is a side effect nobody asked for. An offscreen *application* —
    /// one whose frames a person actually looks at — turns it on.
    ///
    /// The in-process buffer stays underneath either way, so a system
    /// backend that refuses a write does not lose the copy. See
    /// [`Ui::clipboard`](crate::Ui::clipboard).
    #[cfg(feature = "system-clipboard")]
    pub const fn system_clipboard(mut self, system: bool) -> Self {
        self.system_clipboard = system;
        self
    }

    /// The clipboard [`Self::build`] hands the core.
    ///
    /// Split out because which backend it is depends on a feature as well
    /// as on the flag, and `build` should read as the wiring it is rather
    /// than carry a `cfg` in the middle of an argument list.
    fn clipboard(&self) -> Clipboard {
        #[cfg(feature = "system-clipboard")]
        if self.system_clipboard {
            return Clipboard::system_or_memory();
        }
        Clipboard::memory()
    }

    /// Allocate the shared core and the window driver from the sealed
    /// settings.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot run Palantir's pipelines — see
    /// [`DeviceRequirements`](crate::DeviceRequirements). Checked here rather
    /// than left to the first pipeline that trips over it, because a device is
    /// only ever short of a feature its own request forgot to ask for: by the
    /// time one exists, `request_device` has already granted whatever was
    /// asked. The mistake is upstream of this call, so the report belongs at
    /// this boundary and not several layers into the backend.
    pub fn build(self) -> OffscreenHost {
        if let Err(unmet) = self.gpu.requirements_met() {
            panic!("offscreen host device cannot run Palantir: {unmet}");
        }
        let max_texture_dim = self.gpu.max_texture_dim();
        let clipboard = self.clipboard();
        let core = HostCore::new(
            self.gpu,
            max_texture_dim,
            self.shaper.unwrap_or_default(),
            clipboard,
            HostCoreConfig {
                collect_gpu_stats: self.collect_gpu_stats,
                pixel_snap: self.pixel_snap,
            },
        );
        let strategy = if self.retained_target {
            PresentStrategy::DirectAdaptive
        } else {
            // The target's prior contents can't be relied on (a caller may
            // hand in a fresh texture each call), so every frame must fill the
            // whole thing.
            PresentStrategy::BackbufferCopy
        };
        let mut driver = core.driver(OffscreenHost::WINDOW).strategy(strategy);
        if let Some(clock) = self.clock {
            driver = driver.clock(clock);
        }
        let driver = driver.build();
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
    pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            gpu,
            retained_target: false,
            shaper: None,
            collect_gpu_stats: false,
            clock: None,
            pixel_snap: true,
            #[cfg(feature = "system-clipboard")]
            system_clipboard: false,
        }
    }

    /// Mutable access to the window's `Ui` for building scenes.
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    /// Deliver one input event to the window's `Ui`, and report whether it
    /// asks for a repaint.
    ///
    /// The headless peer of the winit host's event pump: an offscreen host
    /// drives a real [`App`], so it needs the door for the pointer and the
    /// keyboard that a windowed one has. The event is stamped with this
    /// host's own clock — the one that also stamps its frames — so a press
    /// between two frames is timed against the other presses rather than
    /// against the frame that carried it.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        let now = self.driver.now();
        self.driver.ui.on_input(event, now)
    }

    /// Run one offscreen application frame against `target`, filling the
    /// supplied texture even when the UI has not changed since the previous
    /// call. The target may be replaced between calls. [`Self::WINDOW`] is
    /// passed to [`App::update`] and [`App::record`], with the same once-only
    /// update and replayable record semantics as [`crate::WinitHost`].
    ///
    /// `system_scale` stands in for the device pixel ratio a platform would
    /// report. The app's own scale multiplies onto it, exactly as in a
    /// window, so a render at 200% asks for it through
    /// [`Ui::set_user_scale`](crate::Ui::set_user_scale) rather than by
    /// doubling this.
    ///
    /// # Panics
    ///
    /// Panics if `system_scale` is non-finite or below `1e-4`, or if the frame
    /// recorded [`Ui::open_window`] / [`Ui::close_window`] — this host has no
    /// window lifecycle.
    pub fn frame<T: App>(
        &mut self,
        target: RenderTarget<'_>,
        system_scale: f32,
        app: &mut T,
    ) -> FrameReport {
        assert!(
            display::scale_factor_is_valid(system_scale),
            "offscreen system scale must be finite and at least {EPS}, got \
             {system_scale}"
        );

        let key = TargetKey::of(target);
        let driver = &mut self.driver;
        driver.note_target(key);
        // No monitor, so no refresh rate to declare.
        let display = driver.display(key.physical, system_scale, None);
        let CpuFrame { report, mode } = self.core.cpu_frame(driver, display, app);
        // Before submitting: a frame that asked for a window it can never get
        // is a caller error, and reporting it against an untouched target
        // keeps the failure clean.
        driver.deny_window_commands();
        self.core.submit(driver, target, mode);
        report
    }

    /// Cloneable handle to the most-recent GPU instrumentation sample —
    /// same handle the `Ui` debug overlay reads from.
    pub const fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.core.resources.diagnostics().gpu_pass_stats
    }
}

/// Peepholes for the visual suite — cache introspection for the
/// format-change test, and a forced full repaint for the pixel damage
/// oracle — and the draw list the `record_pass` benchmark replays. Gated
/// because the first two call `internals`-gated `WgpuBackend` helpers.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::host::offscreen::OffscreenHost;
    #[cfg(feature = "bench")]
    use crate::renderer::render_buffer::RenderBuffer;

    /// Draw list the most recent [`OffscreenHost::frame`]
    /// composed. The `record_pass` benchmark replays the schedule over it
    /// to report the exact step counts behind each timing — a number the
    /// backend never publishes, because counting steps on the production
    /// path would cost what the benchmark exists to measure.
    #[cfg(feature = "bench")]
    pub(crate) const fn last_render_buffer(host: &OffscreenHost) -> &RenderBuffer {
        &host.core.frontend.buffer
    }

    impl OffscreenHost {
        /// Whether the shared backend has built a pipeline set for `format`.
        /// Lets format-change tests confirm a new format materializes its own
        /// pipelines.
        pub fn has_format_pipelines(&self, format: TargetFormat) -> bool {
            self.core.backend.has_format_pipelines(format)
        }

        /// Images resident in the GPU texture cache. Used by the format-change
        /// test to assert the cache survives a new format's pipeline build (no
        /// re-upload).
        pub fn gpu_image_cache_len(&self) -> usize {
            self.core.backend.gpu_image_cache_len()
        }

        /// Paint the next frame in full, as after a swapchain reconfigure:
        /// the reference a partial repaint is compared against.
        pub const fn invalidate_target_contents(&mut self) {
            self.driver.invalidate_target_contents();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::window_driver::PresentPath;
    use crate::internals::record_app::RecordApp;
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::color::RgbaF32;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use glam::UVec2;

    /// The CPU half leaves a frame's output pending and the GPU half
    /// completes it, on a real device: the paint's submit, and the copy
    /// a still frame re-presents the backbuffer with. Each frame runs the
    /// two halves of [`OffscreenHost::frame`] apart so the state between
    /// them can be read.
    #[test]
    fn the_gpu_half_completes_what_the_cpu_half_leaves_pending() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .build();
        let texture = gpu.target("palantir.offscreen.validity", UVec2::new(64, 48));
        let mut app = RecordApp::new(|ui: &mut Ui| {
            Block::new()
                .id_salt("tile")
                .size(10.0)
                .background(Background::fill(RgbaF32::WHITE))
                .show(ui);
        });

        for (frame, paints) in [(0, true), (1, false)] {
            let target = RenderTarget::new(&texture);
            let key = TargetKey::of(target);
            host.driver.note_target(key);
            let display = host.driver.display(key.physical, 1.0, None);
            let CpuFrame { mode, .. } = host.core.cpu_frame(&mut host.driver, display, &mut app);
            assert_eq!(
                matches!(mode, PresentPath::Direct(_) | PresentPath::ViaBackbuffer(_)),
                paints,
                "frame {frame}: {mode:?}",
            );
            if !paints {
                assert_eq!(mode, PresentPath::SkipCopy, "a still frame re-presents");
            }
            assert!(
                !host.driver.output_valid(),
                "frame {frame}: {mode:?} is pending until the GPU half runs",
            );
            host.core.submit(&mut host.driver, target, mode);
            assert!(
                host.driver.output_valid(),
                "frame {frame}: {mode:?} is complete once submitted",
            );
        }
    }
}
