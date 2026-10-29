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
        if let Err(unmet) = self.gpu.requirements_met() {
            panic!("offscreen host device cannot run Palantir: {unmet}");
        }
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
        self.driver.wait_for_captures(self.core.gpu());
    }

    /// Runs one offscreen application frame against `target`, filling it even when the UI is unchanged. The target may change between calls, unless the builder promised [`OffscreenHostBuilder::retained_target`], whose one target keeps what a still frame leaves in it. [`Self::WINDOW`] goes to [`App::update`] and [`App::record`], with the semantics of [`crate::WinitHost`].
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
        self.core.submit(&mut self.driver, target, mode);
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
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::damage::Damage;
    use crate::diagnostics::DebugOverlayConfig;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::wgpu_backend::render_stream::RenderStream;
    use crate::host::window_driver::DriverConfig;
    use crate::host::window_driver::PresentPath;
    use crate::internals::panic_probe;
    use crate::internals::record_app::RecordApp;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::backdrop_filter::BackdropFilter;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_plan::RenderPlan;
    use crate::shape::Shape;
    use crate::ui::resources::UiResources;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;
    use crate::window::cursor_icon::CursorIcon;
    use crate::window::vsync::Vsync;
    use crate::window::window_config::WindowConfig;
    use glam::UVec2;
    use image::{Rgba, imageops};

    /// A driver as this host opens its one window, over `shared`, on a stream with no device.
    fn driver(token: WindowToken, shared: &UiResources) -> WindowDriver {
        WindowDriver::new(
            token,
            shared,
            DriverConfig {
                strategy: PresentStrategy::BackbufferCopy,
                pixel_snap: true,
            },
            RenderStream::detached(shared.render_owners().reserve(), CutoutMode::Shade),
        )
    }

    /// A host with no window lifecycle drains a quiet frame as a windowed one does
    /// (the veto lives a frame either way) and keeps the *levels* the recorder
    /// reads back.
    #[test]
    fn deny_window_commands_accepts_a_quiet_frame_and_clears_the_veto() {
        let shared = UiResources::isolated_mono();
        let mut quiet = driver(WindowToken(1), &shared);
        quiet.ui.keep_open();
        quiet.ui.set_vsync(Vsync::Off);
        quiet.ui.set_cursor(CursorIcon::Text);

        deny_window_commands(&mut quiet);

        assert!(
            !quiet.ui.window_requests().close_vetoed,
            "a veto against a close that was never requested must not persist"
        );
        assert_eq!(
            quiet.ui.vsync(),
            Vsync::Off,
            "a level the host cannot apply is still the one the app set",
        );
        assert_eq!(quiet.ui.window_requests().levels.cursor, CursorIcon::Text);
    }

    #[test]
    #[should_panic(expected = "Ui::open_window(WindowToken(9))")]
    fn deny_window_commands_rejects_an_open() {
        let shared = UiResources::isolated_mono();
        let mut opener = driver(WindowToken(1), &shared);
        opener
            .ui
            .open_window(WindowToken(9), WindowConfig::new("unservable"));

        deny_window_commands(&mut opener);
    }

    #[test]
    #[should_panic(expected = "Ui::close_window(WindowToken(4))")]
    fn deny_window_commands_rejects_a_close() {
        let shared = UiResources::isolated_mono();
        let mut closer = driver(WindowToken(1), &shared);
        closer.ui.close_window(WindowToken(4));

        deny_window_commands(&mut closer);
    }

    /// A manual host stamps each frame with the time it was moved to: a wake asked for 1 s on
    /// is due at 2 s + 1 s = 3 s after `at(2 s)`, and at 3.5 s + 1 s = 4.5 s after a further
    /// `advance(1.5 s)`, which passes the 3 s one.
    #[test]
    fn a_manual_host_stamps_frames_with_the_time_it_was_moved_to() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .time(OffscreenTime::Manual)
            .build();
        let texture = gpu.target("palantir.offscreen.time", UVec2::new(8, 8));
        let mut app =
            RecordApp::new(|ui: &mut Ui| ui.request_repaint_after(Duration::from_secs(1)));
        let mut wake =
            |host: &mut OffscreenHost| host.frame(texture.as_target(), 1.0, &mut app).repaint_after;
        assert_eq!(
            wake(&mut host),
            Some(Duration::from_secs(1)),
            "it starts at zero"
        );
        host.at(Duration::from_secs(2));
        assert_eq!(wake(&mut host), Some(Duration::from_secs(3)));
        host.advance(Duration::from_millis(1500));
        assert_eq!(wake(&mut host), Some(Duration::from_millis(4500)));

        let mut realtime = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .build();
        panic_probe::assert_panics_with("OffscreenTime::Manual", || {
            realtime.advance(Duration::from_secs(1));
        });
    }

    /// A retained target's partial frame paints into the target, loading what it holds: a pixel
    /// the frame does not damage keeps what was written into the texture after the last frame,
    /// so nothing cleared or copied over it, and the damaged 4 × 4 block takes its new color. A
    /// still frame then presents nothing.
    #[test]
    fn a_retained_partial_frame_repaints_the_target_in_place() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .retained_target(true)
            .build();
        host.ui().theme_mut().window_clear = RgbaF32::BLACK;
        let texture = gpu.target("palantir.offscreen.retained", UVec2::new(32, 8));
        let color = Cell::new(RgbaF32::WHITE);
        let mut app = RecordApp::new(|ui: &mut Ui| {
            Block::new()
                .id_salt("tile")
                .size(4.0)
                .chrome(Chrome::fill(color.get()))
                .show(ui);
        });
        let mut frame = |host: &mut OffscreenHost| {
            let target = texture.as_target();
            let spec = host.spec(target, 1.0);
            let CpuFrame { mode, .. } = host.core.record(&mut host.driver, spec, &mut app);
            host.core.submit(&mut host.driver, target, mode);
            mode
        };
        assert!(matches!(
            frame(&mut host),
            PresentPath::Direct(RenderPlan {
                damage: Damage::Full,
                ..
            })
        ));
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: texture.as_target().texture(),
                mip_level: 0,
                origin: wgpu::Origin3d { x: 30, y: 6, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 255, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        color.set(RgbaF32::new(1.0, 0.0, 0.0, 1.0));
        assert!(matches!(
            frame(&mut host),
            PresentPath::Direct(RenderPlan {
                damage: Damage::Partial(_),
                ..
            })
        ));
        let image = texture.read_back();
        assert_eq!(
            *image.get_pixel(1, 1),
            Rgba([255, 0, 0, 255]),
            "the damaged block"
        );
        assert_eq!(
            *image.get_pixel(30, 6),
            Rgba([0, 255, 0, 255]),
            "the kept marker"
        );
        assert_eq!(
            *image.get_pixel(20, 2),
            Rgba([0, 0, 0, 255]),
            "the kept clear"
        );
        assert_eq!(frame(&mut host), PresentPath::SkipNoop, "a still frame");
    }

    /// A retained target never keeps a damage outline: with the outline on, a partial frame
    /// paints through the backbuffer, whole after a direct frame and partial after one through
    /// it, and the first partial frame after the outline goes off still does, as the target
    /// holds the last outline. The frame after that paints in place, and the target then matches
    /// a host that never outlined.
    #[test]
    fn a_retained_target_keeps_no_damage_outline() {
        let gpu = headless_test_gpu();
        let host = |retained: bool| {
            let mut host = OffscreenHost::builder(gpu.handles())
                .shaper(TextShaper::test_mono())
                .retained_target(retained)
                .build();
            host.ui().theme_mut().window_clear = RgbaF32::BLACK;
            host
        };
        let mut retained = host(true);
        let texture = gpu.target("palantir.offscreen.outlined", UVec2::new(32, 16));
        let scene = |red: bool| {
            move |ui: &mut Ui| {
                let color = if red {
                    RgbaF32::new(1.0, 0.0, 0.0, 1.0)
                } else {
                    RgbaF32::WHITE
                };
                Block::new()
                    .id_salt("tile")
                    .size(4.0)
                    .chrome(Chrome::fill(color))
                    .show(ui);
            }
        };
        for (frame, outline, via_backbuffer, partial) in [
            (0, true, false, false),
            (1, true, true, false),
            (2, true, true, true),
            (3, false, true, true),
            (4, false, false, true),
        ] {
            retained.ui().set_debug_overlay(DebugOverlayConfig {
                damage_rect: outline,
                ..DebugOverlayConfig::default()
            });
            let target = texture.as_target();
            let spec = retained.spec(target, 1.0);
            let mut app = RecordApp::new(scene(frame % 2 == 1));
            let CpuFrame { mode, .. } = retained.core.record(&mut retained.driver, spec, &mut app);
            let what = format!("frame {frame}: {mode:?}");
            let (route, plan) = match mode {
                PresentPath::ViaBackbuffer(plan) => (true, plan),
                PresentPath::Direct(plan) => (false, plan),
                PresentPath::SkipCopy | PresentPath::SkipNoop => panic!("{what}"),
            };
            assert_eq!(route, via_backbuffer, "{what}");
            assert_eq!(matches!(plan.damage, Damage::Partial(_)), partial, "{what}");
            retained.core.submit(&mut retained.driver, target, mode);
        }
        let reference = gpu.target("palantir.offscreen.unoutlined", UVec2::new(32, 16));
        host(false).frame(
            reference.as_target(),
            1.0,
            &mut RecordApp::new(scene(false)),
        );
        assert_eq!(texture.read_back(), reference.read_back());
    }

    /// On a retained target that gives no copy, a partial frame whose pane captures paints
    /// through the backbuffer, decided before it builds: as a partial frame over the backbuffer
    /// the first frame left, then whole once a direct frame left the backbuffer behind. A partial
    /// frame whose pane holds its result paints in place.
    #[test]
    fn a_retained_target_without_copies_captures_through_the_backbuffer() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .retained_target(true)
            .build();
        let texture = gpu.uncopied_target("palantir.offscreen.uncopied", UVec2::new(64, 32));
        let scene = |under: bool, away: bool| {
            let color = |on: bool| if on { RgbaF32::WHITE } else { RgbaF32::BLACK };
            move |ui: &mut Ui| {
                Panel::canvas()
                    .id_salt("uncopied")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(Shape::rect(Rect::new(8.0, 8.0, 4.0, 4.0)).fill(color(under)));
                        ui.add_shape(
                            Shape::rect(Rect::new(50.0, 20.0, 4.0, 4.0)).fill(color(away)),
                        );
                        ui.add_shape(
                            Shape::rect(Rect::new(4.0, 4.0, 16.0, 12.0))
                                .backdrop_filter(BackdropFilter::blur(1.0)),
                        );
                    });
            }
        };
        for (under, away, via_backbuffer, partial) in [
            (false, false, true, false),
            (true, false, true, true),
            (true, true, false, true),
            (false, true, true, false),
        ] {
            let target = texture.as_target();
            let spec = host.spec(target, 1.0);
            let mut app = RecordApp::new(scene(under, away));
            let CpuFrame { mode, .. } = host.core.record(&mut host.driver, spec, &mut app);
            let what = format!("under {under}, away {away}: {mode:?}");
            let (route, plan) = match mode {
                PresentPath::ViaBackbuffer(plan) => (true, plan),
                PresentPath::Direct(plan) => (false, plan),
                PresentPath::SkipCopy | PresentPath::SkipNoop => panic!("{what}"),
            };
            assert_eq!(route, via_backbuffer, "{what}");
            assert_eq!(matches!(plan.damage, Damage::Partial(_)), partial, "{what}");
            host.core.submit(&mut host.driver, target, mode);
        }
    }

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
                .chrome(Chrome::fill(RgbaF32::WHITE))
                .show(ui);
        });

        for (frame, paints) in [(0, true), (1, false)] {
            let target = texture.as_target();
            let spec = host.spec(target, 1.0);
            let CpuFrame { mode, .. } = host.core.record(&mut host.driver, spec, &mut app);
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
    /// A frame asks for a region but is never presented, as an outdated swapchain leaves it, and
    /// the window shrinks from 64 × 48 to 32 × 24 before a frame paints. The request carries to
    /// that frame and is resolved against its window: (20, 10) + 30 × 20 clamps to
    /// (20, 10) + 12 × 14, and the image is that crop of the small target.
    #[test]
    fn a_screenshot_request_carries_to_the_frame_that_paints_it() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .build();
        let asked = Cell::new(None);
        let scene = |ui: &mut Ui, ask: bool| {
            Block::new()
                .id_salt("tile")
                .pos((18.0, 8.0))
                .size(10.0)
                .chrome(Chrome::fill(RgbaF32::hex(0xd23f7a)))
                .show(ui);
            if ask {
                asked.set(ui.request_screenshot(Some(Rect::new(20.0, 10.0, 30.0, 20.0))));
            }
        };

        let large = gpu.target("palantir.offscreen.capture.large", UVec2::new(64, 48));
        let spec = host.spec(large.as_target(), 1.0);
        let mut asking = RecordApp::new(|ui: &mut Ui| scene(ui, true));
        let CpuFrame { report, .. } = host.core.record(&mut host.driver, spec, &mut asking);
        assert!(
            report.repaint_requested,
            "the unpainted request asks for a frame"
        );

        let small = gpu.target("palantir.offscreen.capture.small", UVec2::new(32, 24));
        let mut still = RecordApp::new(|ui: &mut Ui| scene(ui, false));
        host.frame(small.as_target(), 1.0, &mut still);
        let target = small.read_back();
        host.wait_for_gpu();
        host.frame(small.as_target(), 1.0, &mut still);
        let shot = host
            .ui()
            .screenshot_taken(asked.get().unwrap())
            .expect("the carried request lands once the device finishes its copy");
        assert_eq!(shot.region(), Rect::new(20.0, 10.0, 12.0, 14.0));
        let image = shot.image();
        assert_eq!(image.size(), UVec2::new(12, 14));
        let crop = imageops::crop_imm(&target, 20, 10, 12, 14).to_image();
        assert_eq!(image.pixels, crop.into_raw());
    }

    /// A carried request whose region the window shrank wholly past has no pixel to copy: the
    /// frame that resolves it copies nothing, and no image lands. (40, 30) + 10 × 10 is inside
    /// 64 × 48 and outside 32 × 24.
    #[test]
    fn a_carried_request_the_window_shrank_past_lands_nothing() {
        let gpu = headless_test_gpu();
        let mut host = OffscreenHost::builder(gpu.handles())
            .shaper(TextShaper::test_mono())
            .build();
        let asked = Cell::new(None);
        let large = gpu.target("palantir.offscreen.capture.large", UVec2::new(64, 48));
        let spec = host.spec(large.as_target(), 1.0);
        let mut asking = RecordApp::new(|ui: &mut Ui| {
            asked.set(ui.request_screenshot(Some(Rect::new(40.0, 30.0, 10.0, 10.0))));
        });
        let _ = host.core.record(&mut host.driver, spec, &mut asking);
        let id = asked
            .get()
            .expect("the region has pixels in the large window");

        let small = gpu.target("palantir.offscreen.capture.small", UVec2::new(32, 24));
        let mut still = RecordApp::new(|_: &mut Ui| {});
        host.frame(small.as_target(), 1.0, &mut still);
        host.wait_for_gpu();
        host.frame(small.as_target(), 1.0, &mut still);
        assert!(host.ui().screenshot_taken(id).is_none());
    }
}
