//! Driver tests, grouped by the decision each pins: present-mode selection, output
//! validity, record-store lifecycle.

mod present_mode_tests {
    use crate::damage::Damage;
    use crate::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    use crate::host::window_driver::PresentPath::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use crate::host::window_driver::PresentStrategy::{BackbufferCopy, DirectAdaptive};
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
    use glam::UVec2;

    use crate::gpu::surface::render_target::TargetFormat;
    use crate::host::window_driver::{PresentPath, PresentStrategy, TargetKey, WindowDriver};
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::frontend::internals::TEST_MAX_TEXTURE_DIM;
    use crate::renderer::render_plan::RenderPlan;

    use crate::damage::Damage;

    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use crate::ui::resources::UiResources;
    use crate::window::cursor_icon::CursorIcon;
    use crate::window::vsync::Vsync;
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_config::WindowConfig;
    use crate::window::window_token::WindowToken;

    fn driver(token: WindowToken, shared: &UiResources) -> WindowDriver {
        WindowDriver::builder(token, shared, true).build()
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

        quiet.deny_window_commands();

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

        opener.deny_window_commands();
    }

    #[test]
    #[should_panic(expected = "Ui::close_window(WindowToken(4))")]
    fn deny_window_commands_rejects_a_close() {
        let shared = UiResources::isolated_mono();
        let mut closer = driver(WindowToken(1), &shared);
        closer.ui.close_window(WindowToken(4));

        closer.deny_window_commands();
    }

    /// A close request reaches the recorder only from the winit host, so the veto
    /// half needs its write door.
    #[cfg(feature = "winit")]
    #[test]
    fn frame_drain_collects_commands_and_applies_close_veto() {
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
        assert_eq!(commands.opens.len(), 1);
        assert_eq!(commands.opens[0].token, opened);
        assert_eq!(
            commands.closes,
            [token],
            "an un-vetoed close becomes this window's own close command"
        );
        assert!(driver.ui.window_requests().commands.opens.is_empty());
        assert!(driver.ui.window_requests().commands.closes.is_empty());
        // Drained by `append`, not `mem::take`, so the recorder keeps its buffers.
        let open_capacity = driver.ui.window_requests().commands.opens.capacity();
        let close_capacity = driver.ui.window_requests().commands.closes.capacity();
        assert!(open_capacity > 0 && close_capacity > 0);

        driver.ui.window_frame_mut().close_requested = true;
        driver.ui.keep_open();
        let mut vetoed = WindowCommands::default();
        driver.drain_window_output(&mut vetoed);
        assert!(vetoed.closes.is_empty());

        let mut settled = WindowCommands::default();
        driver.drain_window_output(&mut settled);
        assert!(settled.closes.is_empty());
        assert!(!driver.ui.window_requests().close_vetoed);
        assert_eq!(
            driver.ui.window_requests().commands.opens.capacity(),
            open_capacity,
            "draining must not hand away the recorder's buffer"
        );
        assert_eq!(
            driver.ui.window_requests().commands.closes.capacity(),
            close_capacity
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
            ime_area: None,
        }
    }

    /// `note_target` is the single gate on retained target state: it reports a
    /// change once per distinct size/format/present-mode and every change clears
    /// the last-frame pixels and the damage baseline. The present-mode axis is what
    /// a runtime vsync toggle rides: the gate is the sole re-reader of the host's
    /// `SurfaceConfiguration`, so a key blind to the field would leave the
    /// swapchain on the old mode.
    #[test]
    fn note_target_tracks_size_format_and_present_mode_and_invalidates_on_change() {
        let shared = UiResources::isolated_mono();
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, true).build();
        let first = TargetKey {
            physical: UVec2::new(64, 48),
            format: TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb),
            vsync: Some(Vsync::On),
        };
        let resized = TargetKey {
            physical: UVec2::new(65, 48),
            ..first
        };
        let reformatted = TargetKey {
            format: TargetFormat::new(wgpu::TextureFormat::Bgra8UnormSrgb),
            ..resized
        };
        let vsync_off = TargetKey {
            vsync: Some(Vsync::Off),
            ..reformatted
        };
        // A texture target carries no mode, yet must read as a change against an
        // otherwise-equal surface.
        let offscreen = TargetKey {
            vsync: None,
            ..vsync_off
        };

        assert!(driver.note_target(first), "the first target is a change");
        assert!(!driver.note_target(first), "an identical target is not");

        for changed in [resized, reformatted, vsync_off, offscreen] {
            driver.output_valid = true;
            driver.backbuffer_fresh = true;
            assert!(driver.note_target(changed));
            assert!(!driver.output_valid, "target change invalidates output");
            assert!(
                !driver.backbuffer_fresh,
                "target change invalidates retained target pixels"
            );
            assert!(!driver.note_target(changed));
        }

        // Repeats after a change must not re-invalidate, or a swapchain window
        // would never keep a damage baseline.
        driver.output_valid = true;
        driver.backbuffer_fresh = true;
        assert!(!driver.note_target(offscreen));
        assert!(driver.output_valid);
        assert!(driver.backbuffer_fresh);
    }

    /// The submit-time "same target" check must ignore the pacing, the one key
    /// field a render target cannot answer for. Regression: `render_to_texture`
    /// asserted the noted key *equals* `TargetKey::of(target)`, which reports
    /// `vsync: None` for a plain texture, so every debug-build swapchain frame
    /// tripped it on first submit.
    #[test]
    fn a_surface_key_describes_its_acquired_texture_whatever_the_pacing() {
        let physical = UVec2::new(3078, 1908);
        let format = TargetFormat::new(wgpu::TextureFormat::Bgra8UnormSrgb);
        let surface = TargetKey {
            physical,
            format,
            vsync: Some(Vsync::On),
        };

        for vsync in [Some(Vsync::On), Some(Vsync::Off), None] {
            let key = TargetKey { vsync, ..surface };
            assert!(
                key.describes(physical, format),
                "{vsync:?} must still describe its own texture"
            );
        }

        // It must still catch a target that is not the one the CPU half ran
        // against.
        assert!(!surface.describes(UVec2::new(3078, 1907), format), "size");
        assert!(
            !surface.describes(
                physical,
                TargetFormat::new(wgpu::TextureFormat::Rgba8UnormSrgb)
            ),
            "format"
        );
        assert_ne!(
            surface,
            TargetKey {
                vsync: Some(Vsync::Off),
                ..surface
            },
            "describes() is deliberately weaker than equality, not a \
             replacement for it"
        );
    }

    #[test]
    fn output_validity_tracks_pending_and_completion() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::new(TEST_MAX_TEXTURE_DIM, shared.gradient_atlas().clone());
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, true).build();
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
    use crate::host::window_driver::WindowDriver;
    use crate::ui::resources::UiResources;
    use crate::window::window_token::WindowToken;

    /// The directory entry belongs to the driver, not whoever built or closed it:
    /// `build` mints it and `Drop` retires it. Registering at builder creation
    /// would leave a token live when a builder is dropped unbuilt
    /// (`Ui::is_window_open` true for a window that never opened), and a retirement
    /// left to the host would need remembering on two close paths.
    #[test]
    fn a_driver_owns_its_directory_entry_from_build_to_drop() {
        let shared = UiResources::isolated_mono();
        let token = WindowToken(11);

        let builder = WindowDriver::builder(token, &shared, true);
        drop(builder);
        assert!(
            !shared.windows().contains(token),
            "a builder that never built owns no window",
        );

        let driver = WindowDriver::builder(token, &shared, true).build();
        assert!(shared.windows().contains(token));
        drop(driver);
        assert!(
            !shared.windows().contains(token),
            "the entry cannot outlive the driver",
        );
    }

    #[test]
    fn window_drivers_have_distinct_render_owners() {
        let shared = UiResources::isolated_mono();
        let first = WindowDriver::builder(WindowToken(1), &shared, true).build();
        let second = WindowDriver::builder(WindowToken(2), &shared, true).build();

        assert_ne!(first.render_owner, second.render_owner);
    }
}

mod record_store_tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::app::App;
    use crate::internals::record_app::RecordApp;

    use crate::host::clock::FixedClock;
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
        let mut window = WindowDriver::builder(token, &shared, false)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        assert_eq!(window.strategy, PresentStrategy::DirectAdaptive);
        assert!(!window.pixel_snap);
        assert_eq!(window.clock.now(), Duration::ZERO);
        let mut app = LifecycleApp::default();

        let _ = window.cpu_frame(
            &mut frontend,
            Display::from_physical(UVec2::new(112, 112), 1.0),
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
        let mut window_a = WindowDriver::builder(WindowToken(1), &shared, true)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let mut window_b = WindowDriver::builder(WindowToken(2), &shared, true)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let display = Display::from_physical(UVec2::new(112, 112), 1.0);

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
    use glam::UVec2;

    use crate::display::user_scale::UserScale;
    use crate::host::window_driver::WindowDriver;
    use crate::primitives::geometry::size::Size;

    use crate::ui::resources::UiResources;
    use crate::window::window_token::WindowToken;

    /// The driver mints its `Display` from what it owns, so a scale the app wrote
    /// during a frame is in the very next one. `pixel_snap` rides the same call.
    #[test]
    fn the_mint_folds_in_the_app_scale_and_the_hosts_snap() {
        let shared = UiResources::isolated_mono();
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, false).build();

        let plain = driver.display(UVec2::new(800, 600), 2.0, None);
        assert_eq!(plain.system_scale, 2.0);
        assert_eq!(plain.user_scale, UserScale::ONE);
        assert_eq!(plain.scale_factor(), 2.0);
        assert!(!plain.pixel_snap, "the snap comes from the host, not here");

        driver.ui.set_user_scale(UserScale::new(1.25).unwrap());
        let zoomed = driver.display(UVec2::new(800, 600), 2.0, None);
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
        let mut first = WindowDriver::builder(WindowToken(1), &shared, true).build();
        let second = WindowDriver::builder(WindowToken(2), &shared, true).build();

        first.ui.set_user_scale(UserScale::new(1.5).unwrap());

        assert_eq!(second.ui.user_scale(), UserScale::new(1.5).unwrap());
        assert_eq!(
            second
                .display(UVec2::new(400, 400), 1.0, None)
                .scale_factor(),
            1.5,
        );
    }
}

/// A partial frame whose shadow cutout plan is stale repaints in full.
mod cutout_census_tests {
    use std::cell::Cell;

    use glam::UVec2;

    use crate::damage::Damage;
    use crate::display::Display;
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    use crate::host::window_driver::{PresentPath, PresentStrategy, WindowDriver};
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
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, true)
            .strategy(PresentStrategy::BackbufferCopy)
            .cutout_mode(CutoutMode::Bake)
            .build();
        let display = Display::from_physical(UVec2::new(600, 200), 1.0);
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
        assert!(damage(3, true).is_partial(), "a tick under 3 cards");
        assert!(matches!(damage(4, true), Damage::Full), "the fourth card");
        assert!(damage(4, false).is_partial(), "a tick under 4 cards");
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

    use crate::gpu::surface::render_target::RenderTarget;
    use crate::gpu::surface::render_target::internals::TargetSpec;
    use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    use crate::host::offscreen::OffscreenHost;
    use crate::internals::record_app::RecordApp;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::size::Size;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::ui::screenshot::Screenshot;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;

    /// 64 × 48 physical pixels: logical 64 × 48 at scale 1, 32 × 24 at scale 2.
    const PHYSICAL: UVec2 = UVec2::new(64, 48);
    /// Frames a screenshot may take to land: the asking frame, then a poll per frame.
    const LANDING_FRAMES: u32 = 8;

    /// What one [`Scene::frame`] did: whether it asks the host for another, and the size of the
    /// screenshot its record pass saw, if one landed.
    #[derive(Clone, Copy, Debug)]
    struct FrameRun {
        repaint: bool,
        recorded: Option<UVec2>,
    }

    /// What one frame of [`Scene`] records beside its blocks.
    #[derive(Clone, Copy, Debug)]
    enum Frame {
        Still,
        /// It asks for a screenshot of the region, or of the window for `None`.
        Asking(Option<Rect>),
    }

    /// A host over one texture of `format`, drawing two blocks of different colors so a crop or
    /// a swapped channel shows.
    struct Scene {
        gpu: HeadlessTestGpuLease,
        host: OffscreenHost,
        texture: wgpu::Texture,
        scale: f32,
    }

    impl Scene {
        fn new(format: wgpu::TextureFormat, scale: f32) -> Self {
            let gpu = headless_test_gpu();
            let host = OffscreenHost::builder(gpu.handles())
                .shaper(TextShaper::test_mono())
                .build();
            let texture = gpu.target_with(TargetSpec {
                label: "palantir.screenshot.test",
                size: PHYSICAL,
                format,
                usage: HeadlessTestGpuLease::TARGET_USAGES,
            });
            Self {
                gpu,
                host,
                texture,
                scale,
            }
        }

        /// One frame.
        fn frame(&mut self, frame: Frame) -> FrameRun {
            let asked = Cell::new(frame);
            let seen = Cell::new(None);
            let mut app = RecordApp::new(|ui: &mut Ui| {
                if let Some(shot) = ui.screenshot_taken() {
                    seen.set(Some(shot.size()));
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
                if let Frame::Asking(region) = asked.get() {
                    ui.request_screenshot(region);
                }
            });
            let repaint = self
                .host
                .frame(RenderTarget::new(&self.texture), self.scale, &mut app)
                .repaint_requested;
            FrameRun {
                repaint,
                recorded: seen.get(),
            }
        }

        /// The target's pixels; the scene is still, so every frame leaves the same ones.
        fn target(&self) -> image::RgbaImage {
            self.gpu.handles().read_back(&self.texture)
        }

        /// Ask for `region`, then run frames until the image lands.
        fn capture(&mut self, region: Option<Rect>) -> Screenshot {
            assert!(
                self.frame(Frame::Asking(region)).repaint,
                "an asking frame asks for the next"
            );
            for _ in 0..LANDING_FRAMES {
                let run = self.frame(Frame::Still);
                if let Some(shot) = self.host.ui().screenshot_taken() {
                    let shot = shot.clone();
                    assert_eq!(
                        run.recorded,
                        Some(shot.size()),
                        "the frame it lands in records, and the app reads it there"
                    );
                    assert!(!run.repaint, "nothing is pending once it landed");
                    let after = self.frame(Frame::Still);
                    assert_eq!(
                        (after.recorded, self.host.ui().screenshot_taken()),
                        (None, None),
                        "a screenshot is an edge of the one frame it lands in"
                    );
                    return shot;
                }
                assert!(run.repaint, "a pending screenshot keeps asking for frames");
            }
            panic!("no screenshot within {LANDING_FRAMES} frames");
        }
    }

    /// `image`'s pixels inside `min..min + size`, row by row.
    fn crop(image: &image::RgbaImage, min: UVec2, size: UVec2) -> Vec<u8> {
        let mut out = Vec::new();
        for y in min.y..min.y + size.y {
            for x in min.x..min.x + size.x {
                out.extend_from_slice(&image.get_pixel(x, y).0);
            }
        }
        out
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
            assert_eq!(shot.size(), PHYSICAL, "{format:?}");
            assert_eq!(shot.scale(), 1.0);
            assert_eq!(shot.region(), Rect::new(0.0, 0.0, 64.0, 48.0));
            assert_eq!(
                shot.pixels(),
                scene.target().as_raw().as_slice(),
                "{format:?}"
            );
            reference.get_or_insert(shot);
        }
        let float = Scene::new(wgpu::TextureFormat::Rgba16Float, 1.0).capture(None);
        assert_eq!(Some(&float), reference.as_ref(), "Rgba16Float");
    }

    /// At scale 2 the logical region (5.25, 3) + (10 × 6) covers physical x 10.5..30.5 and
    /// y 6..18; rounded out, that is (10, 6) + (21 × 12). A region past the 32 × 24 logical
    /// window is clamped first: (28, 20) + (10 × 10) becomes (28, 20) + (4 × 4), physical
    /// (56, 40) + (8 × 8). An empty region gives an empty image.
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
            assert_eq!(shot.size(), size, "{region:?}");
            assert_eq!(shot.scale(), 2.0);
            assert_eq!(
                shot.pixels(),
                crop(&target, min, size).as_slice(),
                "{region:?}"
            );
        }
        let empty = Rect {
            min: Vec2::new(4.0, 4.0),
            size: Size::new(0.0, 3.0),
        };
        let shot = scene.capture(Some(empty));
        assert_eq!(shot.size(), UVec2::ZERO);
        assert!(shot.pixels().is_empty());
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
        assert_eq!(shot.pixels(), scene.target().as_raw().as_slice());
    }
}
