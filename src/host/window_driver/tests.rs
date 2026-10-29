//! Driver tests, grouped by the decision each pins: present-mode selection, output
//! validity, record-store lifecycle.

use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::host::window_driver::{DriverConfig, FrameSpec, PresentStrategy, WindowDriver};
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use glam::UVec2;
use std::time::Duration;

/// A driver's settings as a host opens it.
const fn config(strategy: PresentStrategy, pixel_snap: bool) -> DriverConfig {
    DriverConfig {
        strategy,
        pixel_snap,
    }
}

/// A driver for `token` over `shared`, set by `config`, on a stream with no device behind it that
/// shades every cutout.
fn new_driver(token: WindowToken, shared: &UiResources, config: DriverConfig) -> WindowDriver {
    let stream = RenderStream::detached(shared.render_owners().reserve(), CutoutMode::Shade);
    WindowDriver::new(token, shared, config, stream)
}

/// A texture frame of `physical` at `system_scale`, at time zero.
const fn spec(physical: UVec2, system_scale: f32) -> FrameSpec {
    FrameSpec {
        physical,
        format: TargetFormat::RGBA8_SRGB,
        gives_copy: true,
        system_scale,
        refresh_millihertz: None,
        now: Duration::ZERO,
    }
}

mod present_mode_tests {
    use crate::damage::Damage;
    use crate::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::host::window_driver::PresentPath::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use crate::host::window_driver::PresentStrategy::{BackbufferCopy, DirectAdaptive, Retained};
    use crate::host::window_driver::{PresentPath, present_path};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_plan::RenderPlan;

    /// 100×100 logical surface (10_000 px²) the partial fixtures collapse against:
    /// a `w×h` rect has `coverage = w·h / 10_000`.
    const SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

    fn full() -> RenderPlan {
        RenderPlan {
            clear: RgbaF32::BLACK,
            damage: Damage::Full,
        }
    }
    /// One `Rect` of `w·h` px² built through `collapse_from` against [`SURFACE`],
    /// as the damage engine seals it.
    fn partial(w: f32, h: f32) -> RenderPlan {
        let damage = DamageRegion::collapse_from(
            &[Rect::new(0.0, 0.0, w, h)],
            DEFAULT_PASS_BUDGET_PX,
            SURFACE,
        );
        RenderPlan {
            clear: RgbaF32::BLACK,
            damage: Damage::Partial(damage),
        }
    }
    const DIRECT_FULL: PresentPath = Direct(RenderPlan {
        clear: RgbaF32::BLACK,
        damage: Damage::Full,
    });

    #[test]
    fn backbuffer_copy_fills_target_through_backbuffer() {
        // Fresh target each call: paint via the backbuffer (Full or Partial), skip
        // copies it out.
        for fresh in [false, true] {
            assert_eq!(
                present_path(Some(full()), BackbufferCopy, fresh),
                ViaBackbuffer(full())
            );
            assert_eq!(
                present_path(Some(partial(10.0, 10.0)), BackbufferCopy, fresh),
                ViaBackbuffer(partial(10.0, 10.0))
            );
            assert_eq!(present_path(None, BackbufferCopy, fresh), SkipCopy);
        }
    }

    /// A retained target keeps its pixels, so every painted frame paints into it as planned,
    /// however much it covers and whatever the backbuffer holds, and a still frame presents
    /// nothing.
    #[test]
    fn retained_paints_every_frame_into_the_target() {
        for fresh in [false, true] {
            for plan in [full(), partial(10.0, 10.0), partial(90.0, 90.0)] {
                assert_eq!(present_path(Some(plan), Retained, fresh), Direct(plan));
            }
            assert_eq!(present_path(None, Retained, fresh), SkipNoop);
        }
    }

    #[test]
    fn direct_adaptive_full_and_skip() {
        for fresh in [false, true] {
            assert_eq!(
                present_path(Some(full()), DirectAdaptive, fresh),
                Direct(full())
            );
            assert_eq!(present_path(None, DirectAdaptive, fresh), SkipNoop);
        }
    }

    #[test]
    fn direct_adaptive_small_partial_tracks_backbuffer_freshness() {
        let small = partial(10.0, 10.0);
        assert_eq!(
            present_path(Some(small), DirectAdaptive, true),
            ViaBackbuffer(small)
        );
        assert_eq!(
            present_path(Some(small), DirectAdaptive, false),
            ViaBackbuffer(full())
        );
    }

    #[test]
    fn direct_adaptive_large_partial_promotes_to_direct() {
        let large = partial(80.0, 80.0);
        for fresh in [false, true] {
            assert_eq!(
                present_path(Some(large), DirectAdaptive, fresh),
                DIRECT_FULL
            );
        }
    }

    #[test]
    fn direct_adaptive_promote_threshold_is_strict() {
        // At or below 0.4 stays on the backbuffer path (`>`, not `>=`); just over
        // promotes. 63×63 (0.3969) and 64×64 (0.4096) straddle the line, and 40×100
        // = 4_000 sits on it as `0.4f32`, which only a strict compare keeps.
        for (w, h) in [(63.0, 63.0), (40.0, 100.0)] {
            assert!(
                matches!(
                    present_path(Some(partial(w, h)), DirectAdaptive, true),
                    ViaBackbuffer(_)
                ),
                "{w}×{h}",
            );
        }
        assert_eq!(
            present_path(Some(partial(64.0, 64.0)), DirectAdaptive, true),
            DIRECT_FULL
        );
    }
}

mod output_validity_tests {
    use crate::host::window_driver::tests::new_driver;
    use glam::UVec2;

    use crate::gpu::surface::capture::Capture;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::window_driver::tests::{config, spec};
    use crate::host::window_driver::{
        FrameSpec, PendingScreenshot, PresentPath, PresentStrategy, ScreenshotId, WindowDriver,
    };
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;
    use crate::renderer::render_plan::RenderPlan;
    use std::time::Duration;

    use crate::damage::Damage;

    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use crate::ui::resources::UiResources;
    use crate::window::vsync::Vsync;
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_token::WindowToken;

    fn driver(token: WindowToken, shared: &UiResources) -> WindowDriver {
        new_driver(token, shared, config(PresentStrategy::DirectAdaptive, true))
    }

    /// A close request reaches the recorder only from the winit host, so the veto
    /// half needs its write door.
    #[cfg(feature = "winit")]
    #[test]
    fn frame_drain_collects_commands_and_applies_close_veto() {
        use crate::window::cursor_icon::CursorIcon;
        use crate::window::window_commands::internals::Seen;
        use crate::window::window_config::WindowConfig;

        let shared = UiResources::isolated_mono();
        let token = WindowToken(17);
        let mut driver = driver(token, &shared);
        let opened = WindowToken(18);
        let mut commands = WindowCommands::default();

        driver
            .ui
            .open_window(opened, WindowConfig::new("inspector"));
        driver.ui.set_cursor(CursorIcon::Pointer);
        driver.ui.window_frame_mut().close_requested = true;

        let output = driver.drain_window_output(&mut commands);
        assert_eq!(output.cursor, CursorIcon::Pointer);
        assert_eq!(
            output.vsync,
            Vsync::On,
            "a frame that asked for nothing reports the standing level"
        );
        assert_eq!(
            commands.seen(),
            [Seen::Open(opened, "inspector"), Seen::Close(token)],
            "an un-vetoed close becomes this window's own close command"
        );
        assert!(driver.ui.window_requests().commands.seen().is_empty());
        // Drained by `append`, not `mem::take`, so the recorder keeps its buffer.
        let capacity = driver.ui.window_requests().commands.capacity();
        assert!(capacity > 0);

        driver.ui.window_frame_mut().close_requested = true;
        driver.ui.keep_open();
        let mut vetoed = WindowCommands::default();
        driver.drain_window_output(&mut vetoed);
        assert!(vetoed.seen().is_empty());

        let mut settled = WindowCommands::default();
        driver.drain_window_output(&mut settled);
        assert!(settled.seen().is_empty());
        assert!(!driver.ui.window_requests().close_vetoed);
        assert_eq!(
            driver.ui.window_requests().commands.capacity(),
            capacity,
            "draining must not hand away the recorder's buffer"
        );
    }

    /// Vsync is a level like the cursor, not a one-shot: it survives the drain and
    /// reads back through `Ui::vsync`. Collapsing a repeated level into no
    /// swapchain work is the host's job (`Window::set_vsync`).
    #[test]
    fn vsync_is_a_level_the_drain_copies_and_the_recorder_keeps() {
        let shared = UiResources::isolated_mono();
        let mut driver = driver(WindowToken(3), &shared);
        let mut commands = WindowCommands::default();

        assert_eq!(driver.ui.vsync(), Vsync::On, "vsync is on unless asked off");
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::On);

        driver.ui.set_vsync(Vsync::Off);
        assert_eq!(driver.ui.vsync(), Vsync::Off, "the setter reads back");
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
        assert_eq!(
            driver.drain_window_output(&mut commands).vsync,
            Vsync::Off,
            "the level survives the drain that delivered it",
        );

        driver.ui.set_vsync(Vsync::On);
        driver.ui.set_vsync(Vsync::Off);
        assert_eq!(driver.drain_window_output(&mut commands).vsync, Vsync::Off);
    }

    fn report(plan: Option<RenderPlan>) -> FrameReport {
        FrameReport {
            repaint_requested: false,
            repaint_after: None,
            plan,
            processing: FrameProcessing::SingleLayout,
        }
    }

    /// `note_target` is the single gate on retained target state: each distinct size or format
    /// clears the last-frame pixels and the damage baseline once, and a repeat, or a frame that
    /// moves only the scale, refresh rate or time, clears nothing.
    #[test]
    fn note_target_invalidates_once_per_size_or_format() {
        let shared = UiResources::isolated_mono();
        let mut driver = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let first = spec(UVec2::new(64, 48), 1.0);
        let resized = FrameSpec {
            physical: UVec2::new(65, 48),
            ..first
        };
        let reformatted = FrameSpec {
            format: TargetFormat::new(wgpu::TextureFormat::Bgra8UnormSrgb),
            ..resized
        };
        let uncopyable = FrameSpec {
            gives_copy: false,
            ..reformatted
        };
        let rescaled = FrameSpec {
            system_scale: 2.0,
            refresh_millihertz: Some(120_000),
            now: Duration::from_secs(1),
            ..uncopyable
        };
        // Whether noting `spec` cleared what a kept target holds.
        let mut clears = |spec: &FrameSpec| {
            driver.output_valid = true;
            driver.stream.assume_backbuffer_fresh();
            driver.note_target(spec);
            assert_eq!(driver.output_valid, driver.stream.is_backbuffer_fresh());
            !driver.output_valid
        };
        for (label, spec, want) in [
            ("the first target", first, true),
            ("an identical one", first, false),
            ("a new size", resized, true),
            ("its repeat", resized, false),
            ("a new format", reformatted, true),
            ("its repeat", reformatted, false),
            ("one that gives no copy", uncopyable, true),
            ("a new scale, refresh rate and time", rescaled, false),
        ] {
            assert_eq!(clears(&spec), want, "{label}");
        }
    }

    #[test]
    fn output_validity_tracks_pending_and_completion() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let mut driver = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        assert!(!driver.output_valid, "first frame has no presented output");

        driver.output_valid = true;
        let paint = driver.finish_cpu_frame(
            &mut frontend,
            report(Some(RenderPlan {
                clear: RgbaF32::BLACK,
                damage: Damage::Full,
            })),
        );
        assert!(matches!(paint.mode, PresentPath::Direct(_)));
        assert!(
            !driver.output_valid,
            "paint stays pending until acquire and submit complete"
        );

        // The GPU half completes the paint (`offscreen::tests` runs it); a driver
        // without one has no submit, so the SkipNoop precondition is set here.
        driver.output_valid = true;

        let skip = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip.mode, PresentPath::SkipNoop));
        assert!(
            driver.output_valid,
            "SkipNoop preserves valid target pixels"
        );
        assert!(
            !skip.report.repaint_requested,
            "a still frame asks for nothing"
        );

        // A screenshot still on the device keeps the frames coming, as nothing else wakes the
        // host when it lands; a still frame asks for nothing once it has.
        driver.captures.push_back(PendingScreenshot {
            capture: Capture::never_landing(&headless_test_gpu().device),
            region: Rect::new(0.0, 0.0, 1.0, 1.0),
            scale: 1.0,
            id: ScreenshotId::FIRST,
        });
        let waiting = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(waiting.mode, PresentPath::SkipNoop));
        assert!(
            waiting.report.repaint_requested,
            "a pending screenshot asks for the next frame"
        );
        driver.captures.clear();
        let landed = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(!landed.report.repaint_requested);

        // A copy whose map callback another thread's poll collected runs on that thread, 20 ms
        // later: the wait returns only once it ran, though the device was idle all along.
        driver.captures.push_back(PendingScreenshot {
            capture: Capture::mapped_elsewhere_after(
                &headless_test_gpu().device,
                Duration::from_millis(200),
            ),
            region: Rect::new(0.0, 0.0, 1.0, 1.0),
            scale: 1.0,
            id: ScreenshotId::FIRST,
        });
        driver.wait_for_captures(&headless_test_gpu().handles());
        assert!(!driver.captures[0].capture.is_pending());
        driver.captures.clear();

        driver.strategy = PresentStrategy::BackbufferCopy;
        let skip_copy = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip_copy.mode, PresentPath::SkipCopy));
        assert!(
            !driver.output_valid,
            "SkipCopy stays pending until the copy is submitted"
        );
    }
}

/// What a driver owns while it exists: its place in the app-global window directory
/// and a render-owner id no sibling shares.
mod lifecycle_tests {
    use crate::host::window_driver::PresentStrategy;
    use crate::host::window_driver::tests::config;
    use crate::host::window_driver::tests::new_driver;
    use crate::ui::resources::UiResources;
    use crate::window::window_token::WindowToken;

    /// The directory entry belongs to the driver, not whoever opened or closed it:
    /// `new` mints it and `Drop` retires it, as a retirement left to the host would
    /// need remembering on two close paths.
    #[test]
    fn a_driver_owns_its_directory_entry_from_build_to_drop() {
        let shared = UiResources::isolated_mono();
        let token = WindowToken(11);
        assert!(!shared.windows().contains(token));

        let driver = new_driver(
            token,
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        assert!(shared.windows().contains(token));
        drop(driver);
        assert!(
            !shared.windows().contains(token),
            "the entry cannot outlive the driver",
        );
    }
}

mod record_store_tests {
    use crate::host::window_driver::tests::new_driver;
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::app::App;
    use crate::internals::record_app::RecordApp;

    use crate::display::frame_stamp::FrameStamp;
    use crate::host::window_driver::tests::config;
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::primitives::paint::stroke::Stroke;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;

    use crate::shape::Shape;

    use crate::display::Display;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameProcessing;
    use crate::ui::resources::UiResources;
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;
    use crate::window::window_token::WindowToken;

    #[derive(Debug, PartialEq)]
    struct RecordPayloadSnapshot {
        mesh_vertices: Vec<MeshVertex>,
        mesh_idxs: Vec<u32>,
        polyline_points: Vec<Vec2>,
        polyline_colors: Vec<RgbaF16>,
        text: String,
    }

    #[derive(Debug, Default)]
    struct LifecycleApp {
        updates: Vec<WindowToken>,
        records: Vec<WindowToken>,
    }

    impl App for LifecycleApp {
        fn update(&mut self, win: WindowToken, _ui: &Ui) {
            self.updates.push(win);
        }

        fn record(&mut self, win: WindowToken, _ui: &mut Ui) {
            self.records.push(win);
        }
    }

    fn snapshot(driver: &WindowDriver) -> RecordPayloadSnapshot {
        let store = driver.ui.record_store();
        RecordPayloadSnapshot {
            mesh_vertices: store.meshes.vertices.clone(),
            mesh_idxs: store.meshes.idxs.clone(),
            polyline_points: store.polyline_points.clone(),
            polyline_colors: store.polyline_colors.clone(),
            text: store.interned_text().all().to_owned(),
        }
    }

    /// One window's content: a mesh, a per-point polyline, a `label` and a spinner, under `id`.
    #[derive(Debug)]
    struct Scene<'a> {
        mesh: &'a Mesh,
        points: &'a [Vec2],
        colors: &'a [RgbaF32],
        label: &'a str,
        id: &'static str,
    }

    impl Scene<'_> {
        fn record(&self, ui: &mut Ui) {
            let id = self.id;
            Panel::zstack()
                .id(WidgetId::from_hash(id))
                .size(96.0)
                .show(ui, |ui| {
                    ui.add_shape(Shape::mesh(self.mesh));
                    ui.add_shape(
                        Shape::polyline(self.points, Stroke::new(RgbaF32::WHITE, 3.0))
                            .per_point(self.colors),
                    );
                    let label = ui.intern(self.label);
                    Text::new(label)
                        .id(WidgetId::from_hash((id, "text")))
                        .show(ui);
                    Spinner::new()
                        .id(WidgetId::from_hash((id, "spinner")))
                        .diameter(92.0)
                        .show(ui);
                });
        }
    }

    #[test]
    fn cpu_frame_forwards_token_through_app_lifecycle() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let token = WindowToken(17);
        let mut window = new_driver(
            token,
            &shared,
            config(PresentStrategy::DirectAdaptive, false),
        );
        assert_eq!(window.strategy, PresentStrategy::DirectAdaptive);
        assert!(!window.pixel_snap);
        let mut app = LifecycleApp::default();

        let _ = window.cpu_frame(
            &mut frontend,
            FrameStamp::new(
                Display::from_physical(UVec2::new(112, 112), 1.0),
                Duration::ZERO,
            ),
            &mut app,
        );

        assert_eq!(app.updates, [token], "update runs once");
        assert_eq!(
            app.records,
            [token, token],
            "cold-start warmup and visible pass share the token",
        );
    }

    /// A record pass in one window must not replace the payloads retained by
    /// another window's animation-only frame.
    #[test]
    fn interleaved_window_paint_only_preserves_record_payloads() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let mut window_a = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let mut window_b = new_driver(
            WindowToken(2),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let display = FrameStamp::new(
            Display::from_physical(UVec2::new(112, 112), 1.0),
            Duration::ZERO,
        );

        let mesh_a = Mesh::filled_triangle(
            [
                Vec2::new(12.0, 14.0),
                Vec2::new(72.0, 20.0),
                Vec2::new(26.0, 74.0),
            ],
            RgbaF32::srgb(0.15, 0.65, 0.95),
        );
        let points_a = [
            Vec2::new(8.0, 82.0),
            Vec2::new(28.0, 10.0),
            Vec2::new(68.0, 84.0),
            Vec2::new(88.0, 12.0),
        ];
        let colors_a = [
            RgbaF32::srgb(1.0, 0.0, 0.0),
            RgbaF32::WHITE,
            RgbaF32::srgb(0.0, 1.0, 0.0),
            RgbaF32::srgb(0.0, 0.0, 1.0),
        ];

        let mesh_b = Mesh::filled_polygon(
            &[
                Vec2::new(78.0, 8.0),
                Vec2::new(90.0, 46.0),
                Vec2::new(58.0, 88.0),
                Vec2::new(14.0, 70.0),
                Vec2::new(8.0, 24.0),
            ],
            RgbaF32::srgb(0.9, 0.2, 0.65),
        );
        let points_b = [
            Vec2::new(90.0, 88.0),
            Vec2::new(82.0, 18.0),
            Vec2::new(58.0, 64.0),
            Vec2::new(38.0, 14.0),
            Vec2::new(20.0, 76.0),
            Vec2::new(6.0, 32.0),
        ];
        let colors_b = [
            RgbaF32::WHITE,
            RgbaF32::srgb(0.0, 0.0, 1.0),
            RgbaF32::srgb(0.0, 1.0, 0.0),
            RgbaF32::srgb(1.0, 0.0, 0.0),
            RgbaF32::BLACK,
            RgbaF32::WHITE,
        ];

        let scene_a = Scene {
            mesh: &mesh_a,
            points: &points_a,
            colors: &colors_a,
            label: "retained A",
            id: "window-a",
        };
        let mut app_a = RecordApp::new(|ui| scene_a.record(ui));
        let _ = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        window_a.output_valid = true;
        let retained = snapshot(&window_a);
        assert_eq!(retained.mesh_vertices.len(), 3);
        assert_eq!(retained.polyline_points.len(), 4);
        assert_eq!(retained.text, "retained A");

        let scene_b = Scene {
            mesh: &mesh_b,
            points: &points_b,
            colors: &colors_b,
            label: "window B has a much longer label",
            id: "window-b",
        };
        let mut app_b = RecordApp::new(|ui| scene_b.record(ui));
        let _ = window_b.cpu_frame(&mut frontend, display, &mut app_b);
        window_b.output_valid = true;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}

/// The one place both host-owned display settings reach a frame.
mod display_tests {
    use crate::host::window_driver::tests::new_driver;
    use glam::UVec2;

    use crate::display::user_scale::UserScale;
    use crate::host::window_driver::PresentStrategy;
    use crate::host::window_driver::tests::{config, spec};
    use crate::primitives::geometry::size::Size;

    use crate::ui::resources::UiResources;
    use crate::window::window_token::WindowToken;

    /// The driver mints its stamp from what it owns, so a scale the app wrote
    /// during a frame is in the very next one. `pixel_snap` rides the same call.
    #[test]
    fn the_mint_folds_in_the_app_scale_and_the_hosts_snap() {
        let shared = UiResources::isolated_mono();
        let mut driver = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, false),
        );

        let plain = driver.stamp(&spec(UVec2::new(800, 600), 2.0)).display;
        assert_eq!(plain.system_scale, 2.0);
        assert_eq!(plain.user_scale, UserScale::ONE);
        assert_eq!(plain.scale_factor(), 2.0);
        assert!(!plain.pixel_snap, "the snap comes from the host, not here");

        driver.ui.set_user_scale(UserScale::new(1.25).unwrap());
        let zoomed = driver.stamp(&spec(UVec2::new(800, 600), 2.0)).display;
        assert_eq!(zoomed.system_scale, 2.0, "the platform's half is untouched");
        assert_eq!(zoomed.user_scale, UserScale::new(1.25).unwrap());
        assert_eq!(zoomed.scale_factor(), 2.5);
        assert_eq!(zoomed.logical_size(), Size::new(320.0, 240.0));
        assert_eq!(zoomed.system_logical_size(), Size::new(400.0, 300.0));
    }

    /// The scale is app-global, so two drivers over one `UiResources` mint the same
    /// one.
    #[test]
    fn two_windows_mint_the_one_scale() {
        let shared = UiResources::isolated_mono();
        let mut first = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let second = new_driver(
            WindowToken(2),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );

        first.ui.set_user_scale(UserScale::new(1.5).unwrap());

        assert_eq!(second.ui.user_scale(), UserScale::new(1.5).unwrap());
        assert_eq!(
            second
                .stamp(&spec(UVec2::new(400, 400), 1.0))
                .display
                .scale_factor(),
            1.5,
        );
    }
}

/// A partial frame whose shadow cutout plan is stale repaints in full.
mod cutout_census_tests {
    use crate::gpu::wgpu_backend::render_stream::RenderStream;
    use std::cell::Cell;
    use std::time::Duration;

    use glam::UVec2;

    use crate::damage::Damage;
    use crate::display::Display;
    use crate::display::frame_stamp::FrameStamp;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    use crate::host::window_driver::{DriverConfig, PresentPath, PresentStrategy, WindowDriver};
    use crate::internals::record_app::RecordApp;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::shadow::Shadow;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;
    use crate::shape::Shape;
    use crate::ui::resources::UiResources;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;

    /// Cards whose shadows share the `(8, 4)` key: 3 shade it and 4 pay for its
    /// table (see the damage oracle's `shadows`). A frame repainting only a tick
    /// stays partial at either count; the frame adding the fourth card leaves the
    /// first three showing the shaded form of a key that now reads a table, so it
    /// repaints in full.
    #[test]
    fn a_partial_frame_that_moves_a_kept_table_repaints_in_full() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let mut driver = WindowDriver::new(
            WindowToken(1),
            &shared,
            DriverConfig {
                strategy: PresentStrategy::BackbufferCopy,
                pixel_snap: true,
            },
            RenderStream::detached(shared.render_owners().reserve(), CutoutMode::Bake),
        );
        let display = FrameStamp::new(
            Display::from_physical(UVec2::new(600, 200), 1.0),
            Duration::ZERO,
        );
        let cards = Cell::new(3);
        let tick = Cell::new(false);
        let mut app = RecordApp::new(|ui| {
            Panel::canvas()
                .id_salt("cards")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for card in 0..cards.get() {
                        ui.add_shape(
                            Shape::shadow(Shadow {
                                color: RgbaF32::BLACK,
                                blur: 4.0,
                                ..Shadow::NONE
                            })
                            .at(Rect::new(10.0 + 70.0 * card as f32, 40.0, 40.0, 30.0))
                            .corners(8.0),
                        );
                    }
                    let fill = if tick.get() {
                        RgbaF32::WHITE
                    } else {
                        RgbaF32::BLACK
                    };
                    Block::new()
                        .id_salt("tick")
                        .pos((500.0, 150.0))
                        .size(10.0)
                        .chrome(Chrome::fill(fill))
                        .show(ui);
                });
        });
        let mut damage = |cards_now: usize, tick_now: bool| {
            cards.set(cards_now);
            tick.set(tick_now);
            let frame = driver.cpu_frame(&mut frontend, display, &mut app);
            driver.output_valid = true;
            match frame.mode {
                PresentPath::ViaBackbuffer(plan) => plan.damage,
                mode => panic!("a painting frame through the backbuffer, not {mode:?}"),
            }
        };
        assert!(matches!(damage(3, false), Damage::Full), "the first frame");
        assert!(
            matches!(damage(3, true), Damage::Partial(_)),
            "a tick under 3 cards"
        );
        assert!(matches!(damage(4, true), Damage::Full), "the fourth card");
        assert!(
            matches!(damage(4, false), Damage::Partial(_)),
            "a tick under 4 cards"
        );
        assert!(
            matches!(damage(3, false), Damage::Full),
            "the fourth card gone"
        );
    }
}

/// Screenshots through the public offscreen path: a frame asks, a later frame holds the image,
/// and the image is the frame's pixels, which [`Gpu::read_back`](crate::Gpu::read_back) of the
/// target gives independently.
#[expect(
    clippy::disallowed_types,
    reason = "the targets are made in the formats under test, which only wgpu names"
)]
mod screenshot_tests {
    use std::cell::Cell;

    use glam::{UVec2, Vec2};
    use image::imageops;

    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::headless_window::HeadlessWindow;
    use crate::internals::record_app::RecordApp;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::size::Size;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::window::screenshot::{Screenshot, ScreenshotId};

    /// 64 × 48 physical pixels: logical 64 × 48 at scale 1, 32 × 24 at scale 2.
    const PHYSICAL: UVec2 = UVec2::new(64, 48);

    /// What one [`Scene::frame`] did: whether it asks the host for another, and the screenshot
    /// its record pass saw, if one landed.
    #[derive(Clone, Debug)]
    struct FrameRun {
        repaint: bool,
        recorded: Option<Screenshot>,
    }

    /// What one frame of [`Scene`] records beside its blocks.
    #[derive(Clone, Copy, Debug)]
    enum Frame {
        Still,
        /// It asks for a screenshot of the region, or of the window for `None`.
        Asking(Option<Rect>),
    }

    /// A window in `format`, drawing two blocks of different colors so a crop or a swapped
    /// channel shows.
    struct Scene {
        window: HeadlessWindow,
        /// The id of the last ask, whose image a frame looks for.
        asked: Option<ScreenshotId>,
    }

    impl Scene {
        fn new(format: wgpu::TextureFormat, scale: f32) -> Self {
            let gpu = headless_test_gpu();
            let window = HeadlessWindow::builder(gpu.handles())
                .shaper(TextShaper::test_mono())
                .format(TargetFormat::new(format))
                .open(PHYSICAL, scale);
            Self {
                window,
                asked: None,
            }
        }

        /// The image of the last ask, in the frame it lands in.
        fn landed(&mut self) -> Option<&Screenshot> {
            self.window.ui().screenshot_taken(self.asked?)
        }

        /// One frame.
        fn frame(&mut self, frame: Frame) -> FrameRun {
            let asked = Cell::new(self.asked);
            let seen = Cell::new(None);
            let mut app = RecordApp::new(|ui: &mut Ui| {
                if let Some(shot) = asked.get().and_then(|id| ui.screenshot_taken(id)) {
                    seen.set(Some(shot.clone()));
                }
                Block::new()
                    .id_salt("red")
                    .pos((2.0, 2.0))
                    .size(10.0)
                    .chrome(Chrome::fill(RgbaF32::hex(0xd23f7a)))
                    .show(ui);
                Block::new()
                    .id_salt("blue")
                    .pos((14.0, 6.0))
                    .size(8.0)
                    .chrome(Chrome::fill(RgbaF32::hex(0x2f6fd0)))
                    .show(ui);
                if let Frame::Asking(region) = frame {
                    asked.set(ui.request_screenshot(region));
                }
            });
            let repaint = self.window.frame(&mut app).repaint_requested;
            self.asked = asked.get();
            FrameRun {
                repaint,
                recorded: seen.take(),
            }
        }

        /// The target's pixels; the scene is still, so every frame leaves the same ones.
        fn target(&self) -> image::RgbaImage {
            self.window.read_back()
        }

        /// Ask for `region`, wait for the device, and run the frame the image lands in. Without
        /// the wait, landing is a race between the frame loop and the adapter's copy.
        fn capture(&mut self, region: Option<Rect>) -> Screenshot {
            assert!(
                self.frame(Frame::Asking(region)).repaint,
                "an asking frame asks for the next"
            );
            self.window.wait_for_gpu();
            let run = self.frame(Frame::Still);
            let shot = self
                .landed()
                .expect("a finished copy lands in the next frame")
                .clone();
            assert_eq!(
                run.recorded.as_ref(),
                Some(&shot),
                "the frame it lands in records, and the app reads it there"
            );
            assert!(!run.repaint, "nothing is pending once it landed");
            let after = self.frame(Frame::Still);
            assert_eq!(
                (after.recorded, self.landed()),
                (None, None),
                "a screenshot is an edge of the one frame it lands in"
            );
            shot
        }
    }

    /// The whole window, in RGBA and in BGRA: the image is the target's pixels, at the target's
    /// size and scale, over the whole logical window. A float target, which holds linear light,
    /// gives the same sRGB bytes as the sRGB one.
    #[test]
    fn a_whole_window_screenshot_is_the_frame() {
        let mut reference = None;
        for format in [
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ] {
            let mut scene = Scene::new(format, 1.0);
            let shot = scene.capture(None);
            let image = shot.image();
            assert_eq!(image.size(), PHYSICAL, "{format:?}");
            assert_eq!(shot.scale(), 1.0);
            assert_eq!(shot.region(), Rect::new(0.0, 0.0, 64.0, 48.0));
            assert_eq!(image.pixels, scene.target().into_raw(), "{format:?}");
            reference.get_or_insert(shot);
        }
        let float = Scene::new(wgpu::TextureFormat::Rgba16Float, 1.0).capture(None);
        assert_eq!(Some(&float), reference.as_ref(), "Rgba16Float");
    }

    /// At scale 2 the logical region (5.25, 3) + (10 × 6) covers physical x 10.5..30.5 and
    /// y 6..18; rounded out, that is (10, 6) + (21 × 12). A region past the 32 × 24 logical
    /// window is clamped first: (28, 20) + (10 × 10) becomes (28, 20) + (4 × 4), physical
    /// (56, 40) + (8 × 8). Every target format gives the same bytes. An empty region asks for no
    /// image.
    #[test]
    fn a_region_rounds_out_to_whole_physical_pixels() {
        let mut scene = Scene::new(wgpu::TextureFormat::Rgba8UnormSrgb, 2.0);
        for (region, clamped, min, size) in [
            (
                Rect::new(5.25, 3.0, 10.0, 6.0),
                Rect::new(5.25, 3.0, 10.0, 6.0),
                UVec2::new(10, 6),
                UVec2::new(21, 12),
            ),
            (
                Rect::new(28.0, 20.0, 10.0, 10.0),
                Rect::new(28.0, 20.0, 4.0, 4.0),
                UVec2::new(56, 40),
                UVec2::new(8, 8),
            ),
        ] {
            let shot = scene.capture(Some(region));
            let target = scene.target();
            assert_eq!(shot.region(), clamped);
            let image = shot.image();
            assert_eq!(image.size(), size, "{region:?}");
            assert_eq!(shot.scale(), 2.0);
            let crop = imageops::crop_imm(&target, min.x, min.y, size.x, size.y).to_image();
            assert_eq!(image.pixels, crop.into_raw(), "{region:?}");
            // A BGRA target's region is copied and swapped, a float one's blitted at the
            // region's own size: both read as the sRGB one.
            for format in [
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureFormat::Rgba16Float,
            ] {
                let other = Scene::new(format, 2.0).capture(Some(region));
                assert_eq!(other.image(), image, "{format:?} {region:?}");
            }
        }
        let empty = Rect {
            min: Vec2::new(4.0, 4.0),
            size: Size::new(0.0, 3.0),
        };
        assert_eq!(
            scene.window.ui().request_screenshot(Some(empty)),
            None,
            "an empty region asks nothing"
        );
    }

    /// A still scene skips its frames; one that asks paints in full anyway, so the image is the
    /// whole picture rather than what a partial frame touched.
    #[test]
    fn a_still_frame_that_asks_still_gives_the_whole_picture() {
        let mut scene = Scene::new(wgpu::TextureFormat::Rgba8UnormSrgb, 1.0);
        for _ in 0..3 {
            scene.frame(Frame::Still);
        }
        let shot = scene.capture(None);
        assert_eq!(shot.image().pixels, scene.target().into_raw());
    }

    /// Two regions asked in one frame are two images, each landing in a frame of its own under
    /// the id its ask got, in the order asked, and read by no other id; a region asked again in
    /// the same frame, as a replayed record pass asks, is one ask.
    #[test]
    fn each_ask_lands_under_its_own_id() {
        let mut scene = Scene::new(wgpu::TextureFormat::Rgba8UnormSrgb, 1.0);
        let (left, right) = (
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(10.0, 0.0, 10.0, 10.0),
        );
        let ids = Cell::new(None);
        let mut app = RecordApp::new(|ui: &mut Ui| {
            let mut ask = |region| {
                ui.request_screenshot(Some(region))
                    .expect("a region with pixels")
            };
            let first = ask(left);
            let again = ask(left);
            let second = ask(right);
            ids.set(Some([first, again, second]));
        });
        scene.window.frame(&mut app);
        let [first, again, second] = ids.get().unwrap();
        assert_eq!(first, again, "a region asked again in one frame is one ask");
        assert_ne!(first, second);
        let mut landed = Vec::new();
        for _ in 0..4 {
            scene.window.wait_for_gpu();
            scene.frame(Frame::Still);
            let ui = scene.window.ui();
            for id in [first, second] {
                if let Some(shot) = ui.screenshot_taken(id) {
                    landed.push((id, shot.region()));
                }
            }
        }
        assert_eq!(landed, [(first, left), (second, right)]);
    }
}

mod capture_path_tests {
    use crate::host::window_driver::tests::new_driver;
    use crate::host::window_driver::tests::{config, spec};
    use crate::host::window_driver::{FrameSpec, PresentPath, PresentStrategy};
    use crate::internals::record_app::RecordApp;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::backdrop_filter::BackdropFilter;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;
    use crate::shape::Shape;
    use crate::ui::resources::UiResources;
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    /// The present path of a fresh window's first frame, whole, under one pane at `pane` on a
    /// 100 × 80 target that gives a copy when `gives_copy` is.
    fn first_frame(pane: Rect, gives_copy: bool) -> PresentPath {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let mut driver = new_driver(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let spec = FrameSpec {
            gives_copy,
            ..spec(UVec2::new(100, 80), 1.0)
        };
        driver.note_target(&spec);
        let mut app = RecordApp::new(|ui| {
            Panel::canvas()
                .id_salt("panes")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 4.0, 4.0)).fill(RgbaF32::WHITE));
                    ui.add_shape(Shape::rect(pane).backdrop_filter(BackdropFilter::blur(2.0)));
                });
        });
        let stamp = driver.stamp(&spec);
        driver.cpu_frame(&mut frontend, stamp, &mut app).mode
    }

    /// A whole frame paints straight into the target unless its captures cannot copy out of it,
    /// which gives no copy, or would copy as much as the surface holds: then it paints through
    /// the backbuffer, where its blurs read their level 0 in place.
    #[test]
    fn a_capturing_frame_paints_through_the_backbuffer_where_its_captures_outweigh_it() {
        let small = Rect::new(4.0, 4.0, 20.0, 20.0);
        let whole = Rect::new(0.0, 0.0, 100.0, 80.0);
        for (pane, gives_copy, through, what) in [
            (small, true, false, "a small capture"),
            (small, false, true, "a target that gives no copy"),
            (whole, true, true, "a capture of the whole surface"),
        ] {
            let mode = first_frame(pane, gives_copy);
            assert_eq!(
                matches!(mode, PresentPath::ViaBackbuffer(_)),
                through,
                "{what}: {mode:?}"
            );
            assert!(!matches!(
                mode,
                PresentPath::SkipNoop | PresentPath::SkipCopy
            ));
        }
    }
}
