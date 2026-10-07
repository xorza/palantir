//! [`OffscreenHost`]: the headless peer of [`WinitHost`](crate::WinitHost), rendering into a caller-supplied [`RenderTarget`] with no winit and no swapchain.
//!
//! **One window, no window lifecycle.** A frame that records [`Ui::open_window`] or [`Ui::close_window`] panics rather than silently discarding the request. Window *settings* ([`Ui::set_cursor`](crate::Ui::set_cursor), [`Ui::set_vsync`](crate::Ui::set_vsync)) are accepted but inert.
//!
//! Every rejection is a caller mistake, so `frame` panics rather than returning a `Result`.

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
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::window::window_token::WindowToken;

/// The offscreen analogue of `WinitHost`, rendering into a texture.
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
    retained_target: bool,
    /// `None` until overridden; resolved lazily so an override never pays the font load.
    shaper: Option<TextShaper>,
    collect_gpu_stats: bool,
    /// `None` leaves the driver's default; held because the driver builder needs a `UiResources` that exists only in [`Self::build`].
    clock: Option<Box<dyn Clock>>,
    pixel_snap: bool,
    #[cfg(feature = "system-clipboard")]
    system_clipboard: bool,
}

impl OffscreenHostBuilder {
    /// Which faces this host shapes against. Defaults to [`FontScope::Bundled`], so measurement is machine-independent. Replaces a shaper set by [`Self::shaper`].
    pub fn fonts(mut self, scope: FontScope) -> Self {
        self.shaper = Some(TextShaper::with_fonts(scope));
        self
    }

    /// Replaces the default [`TextShaper`], to share one shaped-buffer cache or preloaded fonts.
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.shaper = Some(shaper);
        self
    }

    /// Opts into GPU timestamp and pipeline-statistics collection; the device needs the wgpu features.
    pub const fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.collect_gpu_stats = collect;
        self
    }

    /// Replaces the realtime clock; a [`FixedClock`](crate::FixedClock) makes screenshots reproducible.
    pub fn clock(mut self, clock: impl Clock + 'static) -> Self {
        self.clock = Some(Box::new(clock));
        self
    }

    /// Promises every frame renders into the *same* texture, so the target keeps the last frame's pixels.
    ///
    /// Off by default: otherwise a partial repaint would leave the rest undefined, so frames go through a retained backbuffer and are copied out. On, small damage repaints straight into the target.
    ///
    /// **Only say it if it is true:** with a fresh texture each call, everything outside the damage is whatever it contained.
    pub const fn retained_target(mut self, retained: bool) -> Self {
        self.retained_target = retained;
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    /// Backs this host's [`Clipboard`] with the OS clipboard instead of the in-process buffer, which stays underneath. Off by default: thumbnailers and compositors never cut text. See [`Ui::clipboard`](crate::Ui::clipboard).
    #[cfg(feature = "system-clipboard")]
    pub const fn system_clipboard(mut self, system: bool) -> Self {
        self.system_clipboard = system;
        self
    }

    fn clipboard(&self) -> Clipboard {
        #[cfg(feature = "system-clipboard")]
        if self.system_clipboard {
            return Clipboard::system_or_memory();
        }
        Clipboard::memory()
    }

    /// Allocates the shared core and the window driver from the sealed settings.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot run Palantir's pipelines, see [`DeviceRequirements`](crate::DeviceRequirements).
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
            // The target's prior contents can't be relied on: fill it whole.
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
    /// The token of this host's one window, as handed to [`App::update`] and [`App::record`].
    pub const WINDOW: WindowToken = WindowToken(0);

    /// Starts building an offscreen host: bundled fonts, no GPU timing, realtime clock, pixel snapping on.
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

    /// Mutable access to the window's `Ui`.
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    /// Delivers one input event to the window's `Ui` and reports whether it asks for a repaint, stamped with the host's clock.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        let now = self.driver.now();
        self.driver.ui.on_input(event, now)
    }

    /// Runs one offscreen application frame against `target`, filling it even when the UI is unchanged; the target may change between calls. [`Self::WINDOW`] goes to [`App::update`] and [`App::record`], with the semantics of [`crate::WinitHost`].
    ///
    /// `system_scale` stands in for the platform's device pixel ratio; the app's scale multiplies onto it (use [`Ui::set_user_scale`](crate::Ui::set_user_scale) for a 200% render).
    ///
    /// # Panics
    ///
    /// Panics if `system_scale` is non-finite or below `1e-4`, or if the frame recorded [`Ui::open_window`] / [`Ui::close_window`].
    pub fn frame<T: App>(
        &mut self,
        target: RenderTarget<'_>,
        system_scale: f32,
        app: &mut T,
    ) -> FrameReport {
        assert!(
            display::scale_factor_is_valid(system_scale),
            "{}, got {system_scale}",
            display::SCALE_RULE,
        );

        let key = TargetKey::of(target);
        let driver = &mut self.driver;
        driver.note_target(key);
        let display = driver.display(key.physical, system_scale, None);
        let CpuFrame { report, mode } = self.core.cpu_frame(driver, display, app);
        // Before submitting, so the failure leaves the target untouched.
        driver.deny_window_commands();
        self.core.submit(driver, target, mode);
        report
    }

    /// Handle to the most recent GPU instrumentation sample, as the debug overlay reads.
    pub const fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.core.resources.diagnostics().gpu_pass_stats
    }
}

/// Peepholes for the visual suite and the `record_pass` benchmark.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::host::offscreen::OffscreenHost;
    #[cfg(feature = "bench")]
    use crate::renderer::render_buffer::RenderBuffer;

    /// Draw list the most recent [`OffscreenHost::frame`] composed; the benchmark replays it for exact step counts.
    #[cfg(feature = "bench")]
    pub(crate) const fn last_render_buffer(host: &OffscreenHost) -> &RenderBuffer {
        &host.core.frontend.buffer
    }

    impl OffscreenHost {
        /// Shades every shadow corner's cutout instead of baking tables; the visual suite's reference.
        pub fn disable_cutout_tables(&mut self) {
            self.driver.disable_cutout_tables();
        }

        /// Draws every shadow as one cell instead of its grid; the visual suite's reference.
        pub fn disable_shadow_grid(&mut self) {
            self.core.backend.disable_shadow_grid();
        }

        /// Whether the shared backend has built a pipeline set for `format`.
        pub fn has_format_pipelines(&self, format: TargetFormat) -> bool {
            self.core.backend.has_format_pipelines(format)
        }

        /// Images resident in the GPU texture cache.
        pub fn gpu_image_cache_len(&self) -> usize {
            self.core.backend.gpu_image_cache_len()
        }

        /// Paints the next frame in full, as after a swapchain reconfigure.
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

    /// The CPU half leaves a frame's output pending and the GPU half completes it.
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
