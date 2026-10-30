//! [`OffscreenHost`]: the headless peer of [`WinitHost`](crate::WinitHost), rendering into a caller-supplied [`RenderTarget`] with no winit and no swapchain.
//!
//! **One window, no window lifecycle.** A frame that records [`Ui::open_window`] or [`Ui::close_window`] panics rather than silently discarding the request. Window *settings* ([`Ui::set_cursor`](crate::Ui::set_cursor), [`Ui::set_vsync`](crate::Ui::set_vsync)) are accepted but inert.
//!
//! Every rejection is a caller mistake, so `frame` panics rather than returning a `Result`.

use crate::app::App;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::display;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::host::clock::RealtimeClock;
use crate::host::core::HostCore;
use crate::host::host_options::HostOptions;
use crate::host::window_driver::{CpuFrame, FrameSpec, PresentStrategy, WindowDriver};
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;
use crate::window::window_commands::{WindowCommand, WindowCommands};
use crate::window::window_token::WindowToken;
use std::time::Duration;

/// The offscreen analog of `WinitHost`, rendering into a texture.
#[derive(Debug)]
pub struct OffscreenHost {
    core: HostCore,
    driver: WindowDriver,
    time: HostTime,
}

/// What an [`OffscreenHost`]'s frames and input are stamped with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffscreenTime {
    /// The wall clock since the host was built.
    Realtime,
    /// A time that moves only on [`OffscreenHost::advance`] and [`OffscreenHost::at`], from zero:
    /// every frame samples the same phase until then, so screenshots reproduce.
    Manual,
}

/// The time source [`OffscreenTime`] chose.
#[derive(Debug)]
enum HostTime {
    Realtime(RealtimeClock),
    Manual(Duration),
}

impl HostTime {
    fn now(&self) -> Duration {
        match self {
            Self::Realtime(clock) => clock.now(),
            Self::Manual(now) => *now,
        }
    }

    #[track_caller]
    fn manual(&mut self) -> &mut Duration {
        match self {
            Self::Manual(now) => now,
            Self::Realtime(_) => {
                panic!(
                    "an offscreen host steps its time only when built with OffscreenTime::Manual"
                )
            }
        }
    }
}

/// Seals offscreen policy before allocating the backend and window driver.
#[derive(Debug)]
#[must_use]
pub struct OffscreenHostBuilder {
    gpu: Gpu,
    options: HostOptions,
    strategy: PresentStrategy,
    time: OffscreenTime,
}

impl OffscreenHostBuilder {
    /// Which faces this host shapes against. Defaults to [`FontScope::Bundled`], so measurement is machine-independent. A shaper set by [`Self::shaper`] wins over it.
    pub const fn fonts(mut self, scope: FontScope) -> Self {
        self.options.fonts = scope;
        self
    }

    /// Shape through `shaper` instead of one loaded from [`Self::fonts`], to share one
    /// shaped-buffer cache or preloaded fonts.
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.options.shaper = Some(shaper);
        self
    }

    /// Opts into GPU timestamp and pipeline-statistics collection; the device needs the wgpu features.
    pub const fn collect_gpu_stats(mut self, collect: bool) -> Self {
        self.options.collect_gpu_stats = collect;
        self
    }

    /// What frames and input are stamped with. [`OffscreenTime::Realtime`] by default;
    /// [`OffscreenTime::Manual`] makes screenshots reproducible.
    pub const fn time(mut self, time: OffscreenTime) -> Self {
        self.time = time;
        self
    }

    /// Promises every frame renders into the *same* texture, so the target keeps the last frame's pixels. A texture of another size or format is a new target, which the next frame paints in full.
    ///
    /// Off by default: otherwise a partial repaint would leave the rest undefined, so frames go through a retained backbuffer and are copied out. On, a frame paints straight into the target, a partial one repainting only its damage, and a still frame leaves the target as it is; a frame that takes a screenshot, outlines its damage, or captures a backdrop from a target without `COPY_SRC` goes through the backbuffer.
    ///
    /// **Only say it if it is true:** with a fresh texture of the same size and format each call, everything outside the damage is whatever it contained.
    pub const fn retained_target(mut self, retained: bool) -> Self {
        self.strategy = if retained {
            PresentStrategy::Retained
        } else {
            // The target's prior contents can't be relied on: fill it whole.
            PresentStrategy::BackbufferCopy
        };
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.options.pixel_snap = pixel_snap;
        self
    }

    /// Backs this host's [`Clipboard`](crate::Clipboard) with the OS clipboard instead of the in-process buffer, which stays underneath. Off by default: thumbnailers and compositors never cut text. See [`Ui::clipboard`](crate::Ui::clipboard).
    #[cfg(feature = "system-clipboard")]
    pub const fn system_clipboard(mut self, system: bool) -> Self {
        self.options.system_clipboard = system;
        self
    }

    /// Allocates the shared core and the window driver from the sealed settings.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot run Palantir's pipelines, see [`DeviceRequirements`](crate::DeviceRequirements).
    pub fn build(self) -> OffscreenHost {
        assert_runs_palantir(&self.gpu);
        let fonts = self.options.fonts;
        let shaper = self
            .options
            .shaper_or_else(|| TextShaper::with_fonts(fonts));
        let core = HostCore::new(self.gpu, &self.options, shaper);
        let driver = core.open(OffscreenHost::WINDOW, self.strategy);
        let time = match self.time {
            OffscreenTime::Realtime => HostTime::Realtime(RealtimeClock::new()),
            OffscreenTime::Manual => HostTime::Manual(Duration::ZERO),
        };
        OffscreenHost { core, driver, time }
    }
}

impl OffscreenHost {
    /// The token of this host's one window, as handed to [`App::update`] and [`App::record`].
    pub const WINDOW: WindowToken = WindowToken(0);

    /// Starts building an offscreen host: bundled fonts, no GPU timing, wall-clock time, pixel snapping on.
    pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            gpu,
            options: HostOptions {
                fonts: FontScope::Bundled,
                shaper: None,
                collect_gpu_stats: false,
                pixel_snap: true,
                #[cfg(feature = "system-clipboard")]
                system_clipboard: false,
            },
            strategy: PresentStrategy::BackbufferCopy,
            time: OffscreenTime::Realtime,
        }
    }

    /// Mutable access to the window's `Ui`.
    pub const fn ui(&mut self) -> &mut Ui {
        &mut self.driver.ui
    }

    /// Delivers one input event to the window's `Ui` and reports whether it asks for a repaint, stamped with the time the next frame reads.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        let now = self.time.now();
        self.driver.ui.on_input(event, now)
    }

    /// Move the host's time forward by `dt`; events fed afterward and the next frame carry it.
    ///
    /// # Panics
    ///
    /// Panics unless the host was built with [`OffscreenTime::Manual`].
    #[track_caller]
    pub fn advance(&mut self, dt: Duration) -> &mut Self {
        *self.time.manual() += dt;
        self
    }

    /// Park the host's time at `time`.
    ///
    /// # Panics
    ///
    /// Panics unless the host was built with [`OffscreenTime::Manual`].
    #[track_caller]
    pub fn at(&mut self, time: Duration) -> &mut Self {
        *self.time.manual() = time;
        self
    }

    /// Block until the device has finished every frame submitted so far, and the copies of their
    /// screenshots are back, also when other threads poll the same device. A screenshot asked
    /// for in one of them then lands in the next frame, so a caller that waits once and runs one
    /// frame reads it in [`Ui::screenshot_taken`](crate::Ui::screenshot_taken).
    pub fn wait_for_gpu(&self) {
        self.core.gpu().wait();
    }

    /// Whether the device this host draws with is lost: a GPU reset, a driver update or a removed
    /// GPU took it. [`Self::frame`] then runs the app and paints nothing, leaving the target as it
    /// is, until the app gives the host a new device through [`Self::recover`].
    ///
    /// The loss shows once a call to the device finds it. A device the app destroys reads lost
    /// after the next [`Self::wait_for_gpu`].
    pub fn is_device_lost(&self) -> bool {
        self.core.gpu().is_lost()
    }

    /// Draw with `gpu` from now on, in place of a lost device: every GPU object the host holds is
    /// built again on it, as [`OffscreenHostBuilder::build`] builds it, and the UI state stays. The
    /// next frame records and paints whole, and [`Ui::device_restored`] is true in it. The
    /// textures of registered images went with the old device, so the app fills them again with
    /// [`ImageHandle::update`](crate::ImageHandle::update). Targets made on the old device are
    /// gone too: the app makes them again on `gpu`.
    ///
    /// # Panics
    ///
    /// Panics if `gpu` cannot run Palantir's pipelines, as [`OffscreenHostBuilder::build`] does.
    pub fn recover(&mut self, gpu: Gpu) {
        assert_runs_palantir(&gpu);
        self.core.attach(gpu, [&mut self.driver]);
    }

    /// Runs one offscreen application frame against `target`, filling it even when the UI is unchanged. The target may change between calls, unless the builder promised [`OffscreenHostBuilder::retained_target`], whose one target keeps what a still frame leaves in it. [`Self::WINDOW`] goes to [`App::update`] and [`App::record`], with the semantics of [`crate::WinitHost`]. On a lost device it paints nothing ([`Self::is_device_lost`]).
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
        let spec = self.spec(target, system_scale);
        let CpuFrame { report, mode } = self.core.record(&mut self.driver, spec, app);
        // Before submitting, so the failure leaves the target untouched.
        deny_window_commands(&mut self.driver);
        if !self.is_device_lost() {
            self.core.submit(&mut self.driver, target, mode);
        }
        report
    }

    /// The frame a texture target describes, at `system_scale` and the host's time.
    #[track_caller]
    fn spec(&self, target: RenderTarget<'_>, system_scale: f32) -> FrameSpec {
        FrameSpec {
            physical: target.size(),
            format: target.format(),
            gives_copy: target.gives_copy(),
            system_scale: display::scale_factor(system_scale),
            refresh_millihertz: None,
            now: self.time.now(),
        }
    }

    /// Handle to the window's most recent GPU instrumentation sample, as the debug overlay reads.
    pub const fn gpu_pass_stats(&self) -> &GpuPassStats {
        self.driver.ui.gpu_pass_stats()
    }
}

/// # Panics
///
/// Panics if `gpu` cannot run Palantir's pipelines.
fn assert_runs_palantir(gpu: &Gpu) {
    if let Err(unmet) = gpu.requirements_met() {
        panic!("offscreen host device cannot run Palantir: {unmet}");
    }
}

/// Drain a frame's window output on a host with no window lifecycle. Levels (cursor, vsync) stay
/// with the recorder and are inert here; commands (open, close) mean nothing unserviced, so they
/// are a caller error.
///
/// # Panics
///
/// Panics if the frame recorded any window open or close request.
fn deny_window_commands(driver: &mut WindowDriver) {
    // Empty by contract, so the drain's `append` moves nothing and nothing allocates.
    let mut denied = WindowCommands::default();
    driver.drain_window_output(&mut denied);
    match denied.first() {
        None => {}
        Some(WindowCommand::Open { token, .. }) => panic!(
            "Ui::open_window({token:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — use \
             WinitHost if the app needs to open windows"
        ),
        Some(WindowCommand::Close(token)) => panic!(
            "Ui::close_window({token:?}) during an offscreen frame: the offscreen \
             host drives one window and has no window lifecycle — drop the \
             host to release it"
        ),
    }
}

/// Peepholes for the visual suite and the benches.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::host::offscreen::{OffscreenHost, OffscreenHostBuilder};
    use crate::host::window_driver::PresentStrategy;
    use crate::renderer::render_buffer::RenderBuffer;

    impl OffscreenHostBuilder {
        /// Present as the winit host does, through [`PresentStrategy::DirectAdaptive`], so a bench
        /// measures what the desktop pays.
        pub(crate) const fn swapchain_present(mut self) -> Self {
            self.strategy = PresentStrategy::DirectAdaptive;
            self
        }
    }

    impl OffscreenHost {
        /// Draw list the most recent [`OffscreenHost::frame`] composed; the benchmark replays it for exact step counts.
        pub(crate) const fn last_render_buffer(&self) -> &RenderBuffer {
            &self.core.frontend.buffer
        }

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

        /// The blurs the last frame ran, one per pane whose cached result went stale.
        pub(crate) const fn backdrop_blurs(&self) -> usize {
            self.core.backend.backdrop_blurs()
        }

        /// Paints the next frame in full, as after a swapchain reconfigure.
        pub const fn invalidate_target_contents(&mut self) {
            self.driver.invalidate_target_contents();
        }
    }
}

#[cfg(test)]
mod tests;
