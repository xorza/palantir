//! [`OffscreenHost`]: the headless peer of [`WinitHost`](crate::WinitHost), rendering into a caller-supplied [`RenderTarget`] with no winit and no swapchain.
//!
//! **One window, no window lifecycle.** A frame that records [`Ui::open_window`] or
//! [`Ui::close_window`] leaves the command in [`OffscreenHost::window_commands`] for the caller.
//! Window *settings* ([`Ui::request_cursor`](crate::Ui::request_cursor),
//! [`Ui::set_vsync`](crate::Ui::set_vsync)) are accepted but inert.
//!
//! Every rejection is a caller mistake, so `frame` panics rather than returning a `Result`.

use crate::display::system_scale::SystemScale;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::FrameTiming;
use crate::host::clock::RealtimeClock;
use crate::host::headless_window::{HeadlessCanvas, HeadlessSettings, HeadlessWindow};
use crate::host::host_core::HostCore;
use crate::host::host_device::HostDevice;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::input_delta::InputDelta;
use crate::primitives::reduction::Reduction;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use crate::theme::color_scheme::ColorScheme;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::ui::frame_report::FrameReport;
use crate::ui::window_frames::present::PresentStrategy;
use crate::window::window_commands::WindowCommand;
use crate::window::window_token::WindowToken;
use std::time::Duration;

/// The offscreen analog of `WinitHost`, rendering into a texture.
#[derive(Debug)]
pub struct OffscreenHost {
    window: HeadlessWindow,
    device: HostDevice,
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

/// Seals offscreen policy before allocating the backend and the window's frames.
#[derive(Debug)]
#[must_use]
pub struct OffscreenHostBuilder {
    gpu: Gpu,
    settings: HeadlessSettings,
    strategy: PresentStrategy,
    time: OffscreenTime,
}

impl OffscreenHostBuilder {
    /// Which faces this host shapes against. Defaults to [`FontScope::Bundled`], so measurement is machine-independent. A shaper set by [`Self::shaper`] wins over it.
    pub const fn fonts(mut self, scope: FontScope) -> Self {
        self.settings.options.fonts = scope;
        self
    }

    /// Shape through `shaper` instead of one loaded from [`Self::fonts`], to share one
    /// shaped-buffer cache or preloaded fonts.
    pub fn shaper(mut self, shaper: TextShaper) -> Self {
        self.settings.options.shaper = Some(shaper);
        self
    }

    /// Opts into GPU timestamp and pipeline-statistics collection; the device needs the wgpu features.
    pub const fn collect_gpu_stats(mut self, collect_gpu_stats: bool) -> Self {
        self.settings.options.timing = FrameTiming::new(collect_gpu_stats);
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
    pub const fn retained_target(mut self, retained_target: bool) -> Self {
        self.strategy = if retained_target {
            PresentStrategy::Retained
        } else {
            // The target's prior contents can't be relied on: fill it whole.
            PresentStrategy::BackbufferCopy
        };
        self
    }

    /// Physical pixels per logical pixel, standing in for the platform's device pixel ratio;
    /// [`SystemScale::ONE`] by default. The app's own scale goes on top through
    /// [`Ui::set_user_scale`](crate::Ui::set_user_scale); the host's
    /// [`set_system_scale`](OffscreenHost::set_system_scale) changes this one later.
    pub const fn system_scale(mut self, scale: SystemScale) -> Self {
        self.settings.system_scale = scale;
        self
    }

    /// The display's refresh rate, which repaint wakes coalesce to; none by default. The host's
    /// [`set_refresh_millihertz`](OffscreenHost::set_refresh_millihertz) changes it later.
    pub const fn refresh_millihertz(mut self, millihertz: u32) -> Self {
        self.settings.refresh_millihertz = Some(millihertz);
        self
    }

    /// The color scheme the system reports, as the winit host reports the desktop's; none by
    /// default, so [`Preference::System`](crate::Preference::System) shows the
    /// fallback. The host's [`set_system_color_scheme`](OffscreenHost::set_system_color_scheme) changes
    /// it later.
    pub const fn system_color_scheme(mut self, scheme: ColorScheme) -> Self {
        self.settings.preferences.color_scheme = Some(scheme);
        self
    }

    /// Whether the system asks for less motion, as the winit host reports the desktop's; `false`
    /// by default. The host's
    /// [`set_system_motion_reduced`](OffscreenHost::set_system_motion_reduced) changes it later.
    pub const fn system_motion_reduced(mut self, reduced: bool) -> Self {
        self.settings.preferences.reductions.motion = Reduction::of(reduced);
        self
    }

    /// Whether the system asks for less transparency, as the winit host reports the desktop's;
    /// `false` by default. The host's
    /// [`set_system_transparency_reduced`](OffscreenHost::set_system_transparency_reduced)
    /// changes it later.
    pub const fn system_transparency_reduced(mut self, reduced: bool) -> Self {
        self.settings.preferences.reductions.transparency = Reduction::of(reduced);
        self
    }

    /// Whether the system's power-saver mode is on, as the winit host reports the desktop's;
    /// `false` by default. The host's
    /// [`set_system_power_reduced`](OffscreenHost::set_system_power_reduced) changes it later.
    pub const fn system_power_reduced(mut self, reduced: bool) -> Self {
        self.settings.preferences.reductions.power = Reduction::of(reduced);
        self
    }

    /// Whether axis-aligned paint edges snap to physical pixels.
    pub const fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.settings.options.pixel_snap = pixel_snap;
        self
    }

    /// Backs this host's [`Clipboard`](crate::Clipboard) with the OS clipboard instead of the in-process buffer, which stays underneath. Off by default: thumbnailers and compositors never cut text. See [`Ui::clipboard`](crate::Ui::clipboard).
    #[cfg(feature = "system-clipboard")]
    pub const fn system_clipboard(mut self, system_clipboard: bool) -> Self {
        self.settings.options.system_clipboard = system_clipboard;
        self
    }

    /// Allocates the shared core and the window's frames from the sealed settings.
    ///
    /// # Panics
    ///
    /// Panics if the device cannot run Palantir's pipelines, see [`DeviceRequirements`](crate::DeviceRequirements).
    pub fn build(self) -> OffscreenHost {
        let options = &self.settings.options;
        let fonts = options.fonts;
        let shaper = options.shaper_or_else(|| TextShaper::with_fonts(fonts));
        let mut core = HostCore::new(self.gpu, options, shaper);
        let frames = core.open(OffscreenHost::WINDOW, self.strategy);
        let HostCore { cpu, device } = core;
        let window = HeadlessWindow::open(cpu, frames, &self.settings);
        let time = match self.time {
            OffscreenTime::Realtime => HostTime::Realtime(RealtimeClock::new()),
            OffscreenTime::Manual => HostTime::Manual(Duration::ZERO),
        };
        OffscreenHost {
            window,
            device,
            time,
        }
    }
}

impl OffscreenHost {
    /// The token of this host's one window, as handed to [`App::record`].
    pub const WINDOW: WindowToken = WindowToken(0);

    /// Starts building an offscreen host: bundled fonts, no GPU timing, wall-clock time, pixel snapping on.
    pub const fn builder(gpu: Gpu) -> OffscreenHostBuilder {
        OffscreenHostBuilder {
            gpu,
            settings: HeadlessSettings::new(FontScope::Bundled),
            strategy: PresentStrategy::BackbufferCopy,
            time: OffscreenTime::Realtime,
        }
    }

    /// The window's `Ui`, for what no method here reads.
    pub const fn ui(&self) -> &Ui {
        self.window.frames.ui()
    }

    /// The window's `Ui`, to change what no method here changes.
    pub const fn ui_mut(&mut self) -> &mut Ui {
        self.window.frames.ui_mut()
    }

    /// Delivers one input event to the window's `Ui` and reports whether it asks for a repaint, stamped with the time the next frame reads.
    pub fn on_input(&mut self, event: InputEvent<'_>) -> InputDelta {
        let now = self.time.now();
        self.window.frames.on_input(event, now)
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

    /// Report `scheme` as the system's, as the winit host reports the desktop's; `None`, the
    /// start, reports nothing, so [`Preference::System`](crate::Preference::System)
    /// shows the fallback. The next frame shows the resolved scheme.
    pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.color_scheme = scheme);
        self
    }

    /// Report the system as asking for less motion, or not, as the winit host reports the
    /// desktop's; `false` at the start. See [`Ui::is_motion_reduced`].
    pub fn set_system_motion_reduced(&mut self, reduced: bool) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.reductions.motion = Reduction::of(reduced));
        self
    }

    /// Report the system as asking for less transparency, or not, as the winit host reports the
    /// desktop's; `false` at the start. See [`Ui::is_transparency_reduced`].
    pub fn set_system_transparency_reduced(&mut self, reduced: bool) -> &mut Self {
        self.window.set_preferences(|preferences| {
            preferences.reductions.transparency = Reduction::of(reduced);
        });
        self
    }

    /// Report the system's power-saver mode as on, or not, as the winit host reports the
    /// desktop's; `false` at the start. See [`Ui::is_power_reduced`].
    pub fn set_system_power_reduced(&mut self, reduced: bool) -> &mut Self {
        self.window
            .set_preferences(|preferences| preferences.reductions.power = Reduction::of(reduced));
        self
    }

    /// Block until the device has finished every frame submitted so far, and the copies of their
    /// screenshots are back, also when other threads poll the same device. A screenshot asked
    /// for in one of them then lands in the next frame, so a caller that waits once and runs one
    /// frame reads it in [`Ui::screenshot_taken`](crate::Ui::screenshot_taken). Returns how the
    /// wait ended: [`GpuWait::Done`], or why the copies did not come back.
    pub fn wait_for_gpu(&self) -> GpuWait {
        self.device.wait_for_gpu(&self.window.frames)
    }

    /// The windows the last frame opened or closed with [`Ui::open_window`] and
    /// [`Ui::close_window`], in the order called: this host has one window and opens or closes
    /// none, so an app that drives other windows reads them here.
    pub fn window_commands(&self) -> &[WindowCommand] {
        self.window.commands().as_slice()
    }

    /// Whether the device this host draws with is lost: a GPU reset, a driver update or a removed
    /// GPU took it. [`Self::frame`] then runs the app and paints nothing, leaving the target as it
    /// is, until the app gives the host a new device through [`Self::recover`].
    ///
    /// The loss shows once a call to the device finds it. A device the app destroys reads lost
    /// after the next [`Self::wait_for_gpu`].
    pub fn is_device_lost(&self) -> bool {
        self.device.gpu().is_lost()
    }

    /// Draw with `gpu` from now on, in place of a lost device: every GPU object the host holds is
    /// built again on it, as [`OffscreenHostBuilder::build`] builds it, and the UI state stays. The
    /// next frame records and paints whole. The textures of registered images went with the old
    /// device, so the app fills each image that is not
    /// [resident](crate::ImageHandle::is_resident) again with
    /// [`ImageHandle::update`](crate::ImageHandle::update). Targets made on the old device are
    /// gone too: the app makes them again on `gpu`.
    ///
    /// # Panics
    ///
    /// Panics if `gpu` cannot run Palantir's pipelines, as [`OffscreenHostBuilder::build`] does.
    pub fn recover(&mut self, gpu: Gpu) {
        self.window.recover(&mut self.device, gpu);
    }

    /// Report `scale` physical pixels per logical pixel from the next frame on, standing in for
    /// the platform's device pixel ratio, as a move to a monitor of another density changes it.
    pub const fn set_system_scale(&mut self, scale: SystemScale) -> &mut Self {
        self.window.system_scale = scale;
        self
    }

    /// Report the display's refresh rate, which repaint wakes coalesce to, from the next frame
    /// on; `None` reports none.
    pub const fn set_refresh_millihertz(&mut self, millihertz: Option<u32>) -> &mut Self {
        self.window.refresh_millihertz = millihertz;
        self
    }

    /// Runs one offscreen application frame against `target`, filling it even when the UI is unchanged. The target may change between calls, unless the builder promised [`OffscreenHostBuilder::retained_target`], whose one target keeps what a still frame leaves in it. [`Self::WINDOW`] goes to [`App::record`], with the semantics of [`crate::WinitHost`]. On a lost device it paints nothing ([`Self::is_device_lost`]).
    ///
    /// The frame runs at the host's [system scale](Self::set_system_scale); the app's scale
    /// multiplies onto it (use [`Ui::set_user_scale`](crate::Ui::set_user_scale) for a 200%
    /// render).
    ///
    /// The caller paces the frames, as the report asks: the next when
    /// [`repaint_requested`](FrameReport::repaint_requested) or
    /// [`repaint_at`](FrameReport::repaint_at) says, and no sooner than
    /// [`earliest_repaint_at`](FrameReport::earliest_repaint_at) while power is reduced.
    pub fn frame(&mut self, target: RenderTarget<'_>, app: &mut impl App) -> FrameReport {
        let now = self.time.now();
        self.window.paint_frame(
            app,
            HeadlessCanvas {
                device: &mut self.device,
                target,
            },
            now,
        )
    }
}

/// Peepholes for the visual suite and the benches.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::damage::Damage;
    use crate::gpu::atlas_memory::AtlasMemory;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::host::offscreen::{OffscreenHost, OffscreenHostBuilder};
    use crate::primitives::geometry::rect::Rect;
    use crate::renderer::render_buffer::RenderBuffer;
    use crate::ui::window_frames::present::PresentStrategy;

    #[cfg(test)]
    use crate::gpu::resource::wgpu_effect_store::WgpuEffectStore;
    #[cfg(test)]
    use crate::gpu::surface::render_target::RenderTarget;
    #[cfg(test)]
    use crate::ui::app::App;
    #[cfg(test)]
    use crate::ui::window_frames::WindowFrames;
    #[cfg(test)]
    use crate::ui::window_frames::present::present_facts::PaintPath;
    #[cfg(test)]
    use crate::ui::window_frames::present::{BuiltFrame, CpuFrame, FrameSpec, TargetKey};

    impl OffscreenHostBuilder {
        /// Present as the winit host does, through `PresentStrategy::DirectAdaptive`, so a bench
        /// measures what the desktop pays.
        pub const fn swapchain_present(mut self) -> Self {
            self.strategy = PresentStrategy::DirectAdaptive;
            self
        }
    }

    impl OffscreenHost {
        /// Have the next frame paint `damage` over a change of `changed`, whatever damage it finds.
        pub(crate) fn force_next_frame(&mut self, damage: Damage, changed: &[Rect]) {
            self.window.frames.force_next_frame(damage, changed);
        }

        /// The frame `target` describes at the host's time, as [`Self::frame`] runs it.
        #[cfg(test)]
        pub(super) fn spec(&self, target: RenderTarget<'_>) -> FrameSpec {
            self.window.spec(TargetKey::of(target), self.time.now())
        }

        /// The CPU half of a frame that paints, as [`Self::frame`] runs it, for a test of a frame
        /// that is never presented.
        #[cfg(test)]
        pub(super) fn record(&mut self, spec: FrameSpec, app: &mut impl App) -> CpuFrame<'_> {
            self.device
                .frame(&mut self.window.cpu, &mut self.window.frames, spec, app)
        }

        /// Both halves of a frame onto `target`, as [`Self::frame`] runs them whatever the device
        /// holds, with `between` run after the first: for a test that reads or holds the frame
        /// between them.
        #[cfg(test)]
        pub(super) fn halves<R>(
            &mut self,
            target: RenderTarget<'_>,
            app: &mut impl App,
            between: impl FnOnce(&BuiltFrame<'_>, &WindowFrames) -> R,
        ) -> R {
            let spec = self.spec(target);
            let CpuFrame { built, .. } =
                self.device
                    .frame(&mut self.window.cpu, &mut self.window.frames, spec, app);
            let read = between(&built, &self.window.frames);
            self.device.submit(&mut self.window.frames, target, built);
            read
        }

        /// The store the host's effects build in.
        #[cfg(test)]
        pub(crate) fn effect_store(&self) -> &WgpuEffectStore {
            self.device.backend().effect_store()
        }

        /// A frame's path onto `target`, `None` where it draws nothing.
        #[cfg(test)]
        pub(super) fn frame_path(
            &mut self,
            target: RenderTarget<'_>,
            app: &mut impl App,
        ) -> Option<PaintPath> {
            self.halves(target, app, |built, _| built.path())
        }

        /// Draw list the most recent [`OffscreenHost::frame`] composed; the benchmark replays it for exact step counts.
        pub(crate) const fn last_render_buffer(&self) -> &RenderBuffer {
            &self.window.cpu.frontend.buffer
        }

        /// Shades every shadow corner's cutout instead of baking tables; the visual suite's reference.
        pub fn disable_cutout_tables(&mut self) {
            self.window.frames.disable_cutout_tables();
        }

        /// Draws every shadow as one quad instead of its template; the visual suite's reference.
        pub fn disable_shadow_template(&mut self) {
            self.device.backend_mut().disable_shadow_template();
        }

        /// Draws every box, circle and triangle as one quad instead of its template; the visual
        /// suite's reference.
        pub fn disable_shape_template(&mut self) {
            self.device.backend_mut().disable_shape_template();
        }

        /// Whether the shared backend has built a pipeline set for `format`.
        pub fn has_format_pipelines(&self, format: TargetFormat) -> bool {
            self.device.backend().has_format_pipelines(format)
        }

        /// Every atlas's memory on the host's device.
        pub(crate) fn atlas_memory(&self) -> AtlasMemory {
            self.device
                .backend()
                .atlas_memory(self.window.cpu.frontend.row_table())
        }

        /// Images resident in the GPU texture cache.
        pub fn gpu_image_cache_len(&self) -> usize {
            self.device.backend().gpu_image_cache_len()
        }

        /// The blurs the last frame ran, one per backdrop whose cached result went stale.
        pub(crate) const fn backdrop_blurs(&self) -> usize {
            self.device.backend().backdrop_blurs()
        }

        /// Paints the next frame in full, as after a swapchain reconfigure.
        pub const fn invalidate_target_contents(&mut self) {
            self.window.frames.invalidate_target_contents();
        }
    }
}

#[cfg(test)]
mod tests;
