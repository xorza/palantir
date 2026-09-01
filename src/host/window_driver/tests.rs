//! Driver tests, grouped by the decision each one pins: present-mode
//! selection, output validity, and the record-store lifecycle.

mod present_mode_tests {
    use crate::host::window_driver::PresentMode::{Direct, SkipCopy, SkipNoop, ViaBackbuffer};
    use crate::host::window_driver::PresentStrategy::{BackbufferCopy, DirectAdaptive};
    use crate::host::window_driver::{PresentMode, present_mode};
    use crate::primitives::color::Color;
    use crate::primitives::rect::Rect;
    use crate::renderer::render_plan::RenderPlan;
    use crate::scene::damage::Damage;
    use crate::scene::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};

    /// 100×100 logical surface (10_000 px²) the partial fixtures collapse
    /// against, so a `w×h` damage rect carries `coverage = w·h / 10_000`.
    const SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

    fn full() -> Option<RenderPlan> {
        Some(RenderPlan {
            clear: Color::BLACK,
            damage: Damage::Full,
        })
    }
    /// One `Rect` of `w·h` px², built through `collapse_from` against
    /// [`SURFACE`] so its coverage is `w·h / 10_000` — exactly what the
    /// damage engine seals in the real path.
    fn partial(w: f32, h: f32) -> Option<RenderPlan> {
        let damage = DamageRegion::collapse_from(
            &[Rect::new(0.0, 0.0, w, h)],
            DEFAULT_PASS_BUDGET_PX,
            SURFACE,
        );
        Some(RenderPlan {
            clear: Color::BLACK,
            damage: Damage::Partial(damage),
        })
    }
    const DIRECT_FULL: PresentMode = Direct(RenderPlan {
        clear: Color::BLACK,
        damage: Damage::Full,
    });

    #[test]
    fn backbuffer_copy_fills_target_through_backbuffer() {
        // Fresh target each call: paint via the backbuffer (the requested plan,
        // Full or Partial), skip copies it out — the whole target is filled.
        // Backbuffer freshness is irrelevant here (every frame touches it).
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), BackbufferCopy, fresh),
                ViaBackbuffer(full().unwrap())
            );
            assert_eq!(
                present_mode(partial(10.0, 10.0), BackbufferCopy, fresh),
                ViaBackbuffer(partial(10.0, 10.0).unwrap())
            );
            assert_eq!(present_mode(None, BackbufferCopy, fresh), SkipCopy);
        }
    }

    #[test]
    fn direct_adaptive_full_and_skip() {
        // A whole-surface repaint goes straight in; a skip is a noop. Neither
        // depends on backbuffer freshness.
        for fresh in [false, true] {
            assert_eq!(
                present_mode(full(), DirectAdaptive, fresh),
                Direct(full().unwrap())
            );
            assert_eq!(present_mode(None, DirectAdaptive, fresh), SkipNoop);
        }
    }

    #[test]
    fn direct_adaptive_small_partial_tracks_backbuffer_freshness() {
        // 10×10 = 100 px² ⇒ coverage 0.01, well under the 0.4 promote line.
        let small = partial(10.0, 10.0);
        // Fresh: the backbuffer mirrors the target, so paint just the region.
        assert_eq!(
            present_mode(small, DirectAdaptive, true),
            ViaBackbuffer(small.unwrap())
        );
        // Stale (after a direct frame): resync with one full repaint first.
        assert_eq!(
            present_mode(small, DirectAdaptive, false),
            ViaBackbuffer(full().unwrap())
        );
    }

    #[test]
    fn direct_adaptive_large_partial_promotes_to_direct() {
        // 80×80 = 6_400 px² ⇒ coverage 0.64 > 0.4: a large partial repaints
        // direct (dropping the copy) regardless of backbuffer freshness.
        let large = partial(80.0, 80.0);
        for fresh in [false, true] {
            assert_eq!(present_mode(large, DirectAdaptive, fresh), DIRECT_FULL);
        }
    }

    #[test]
    fn direct_adaptive_promote_threshold_is_strict() {
        // Coverage at-or-below 0.4 stays on the backbuffer path (`>`, not `>=`);
        // just over promotes. 63×63 = 3_969 (0.3969) vs 64×64 = 4_096 (0.4096) —
        // straddling the 0.4 line.
        assert!(matches!(
            present_mode(partial(63.0, 63.0), DirectAdaptive, true),
            ViaBackbuffer(_)
        ));
        assert_eq!(
            present_mode(partial(64.0, 64.0), DirectAdaptive, true),
            DIRECT_FULL
        );
    }
}

mod output_validity_tests {
    use glam::UVec2;

    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentMode, PresentStrategy, TargetKey, WindowDriver};
    use crate::primitives::color::Color;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_plan::RenderPlan;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::scene::damage::Damage;
    use crate::text::shaper::TextShaper;
    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use crate::window::cursor_icon::CursorIcon;
    use crate::window::vsync::Vsync;
    use crate::window::window_config::WindowConfig;
    use crate::window::window_token::WindowToken;

    fn driver(token: WindowToken, shared: &HostShared) -> WindowDriver {
        WindowDriver::builder(token, shared, true).build()
    }

    /// A host with no window lifecycle drains a quiet frame exactly as a
    /// windowed one does — the veto lives a frame either way — and keeps
    /// the *levels* the recorder reads back, which is the half of the
    /// output it is allowed to leave inert.
    #[test]
    fn deny_window_commands_accepts_a_quiet_frame_and_clears_the_veto() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
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
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut opener = driver(WindowToken(1), &shared);
        opener
            .ui
            .open_window(WindowToken(9), WindowConfig::new("unservable"));

        opener.deny_window_commands();
    }

    #[test]
    #[should_panic(expected = "Ui::close_window(WindowToken(4))")]
    fn deny_window_commands_rejects_a_close() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut closer = driver(WindowToken(1), &shared);
        closer.ui.close_window(WindowToken(4));

        closer.deny_window_commands();
    }

    fn report(plan: Option<RenderPlan>) -> FrameReport {
        FrameReport {
            repaint_requested: false,
            repaint_after: None,
            plan,
            processing: FrameProcessing::SingleLayout,
        }
    }

    /// `note_target` is the single gate on retained target state: it reports a
    /// change exactly once per distinct size/format/present-mode, and every
    /// change clears the last-frame pixels and the damage baseline.
    ///
    /// The present-mode axis is what a runtime vsync toggle rides: applying
    /// one only rewrites the host's `SurfaceConfiguration`, and this gate is
    /// the sole thing that re-reads it, so a key blind to the field would
    /// leave the swapchain on the old mode forever.
    #[test]
    fn note_target_tracks_size_format_and_present_mode_and_invalidates_on_change() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, true).build();
        let first = TargetKey {
            physical: UVec2::new(64, 48),
            format: wgpu::TextureFormat::Rgba8Unorm,
            present_mode: Some(wgpu::PresentMode::AutoVsync),
        };
        let resized = TargetKey {
            physical: UVec2::new(65, 48),
            ..first
        };
        let reformatted = TargetKey {
            format: wgpu::TextureFormat::Bgra8Unorm,
            ..resized
        };
        let vsync_off = TargetKey {
            present_mode: Some(wgpu::PresentMode::AutoNoVsync),
            ..reformatted
        };
        // A texture target is never presented, so it carries no mode at all —
        // and must still read as a change against an otherwise-equal surface.
        let offscreen = TargetKey {
            present_mode: None,
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

        // Repeats after a change must not re-invalidate: a swapchain window
        // paints every frame against a steady target and would never keep a
        // damage baseline if they did.
        driver.output_valid = true;
        driver.backbuffer_fresh = true;
        assert!(!driver.note_target(offscreen));
        assert!(driver.output_valid);
        assert!(driver.backbuffer_fresh);
    }

    /// The submit-time "same target" check must ignore `present_mode`, which
    /// is the one field of the key a `wgpu::Texture` cannot answer for.
    ///
    /// The regression: `render_to_texture` asserted the noted key *equals*
    /// `TargetKey::of(target)`, and `of` reports `present_mode: None` because
    /// a plain texture has no swapchain. Once a surface key started carrying
    /// `Some(..)`, the two could never be equal — every debug-build swapchain
    /// frame tripped it on the first submit.
    #[test]
    fn a_surface_key_describes_its_acquired_texture_whatever_the_present_mode() {
        let physical = UVec2::new(3078, 1908);
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let surface = TargetKey {
            physical,
            format,
            present_mode: Some(wgpu::PresentMode::AutoVsync),
        };

        // An acquired swapchain texture reports size + format and nothing
        // else; every present mode describes it, including no mode at all.
        for present_mode in [
            Some(wgpu::PresentMode::AutoVsync),
            Some(wgpu::PresentMode::AutoNoVsync),
            None,
        ] {
            let key = TargetKey {
                present_mode,
                ..surface
            };
            assert!(
                key.describes(physical, format),
                "{present_mode:?} must still describe its own texture"
            );
        }

        // What it must still catch: the target the GPU half was handed is
        // genuinely not the one the CPU half ran against.
        assert!(!surface.describes(UVec2::new(3078, 1907), format), "size");
        assert!(
            !surface.describes(physical, wgpu::TextureFormat::Rgba8Unorm),
            "format"
        );
        // And the mode axis stays live for `note_target`'s own equality —
        // that gate is what reconfigures the swapchain.
        assert_ne!(
            surface,
            TargetKey {
                present_mode: Some(wgpu::PresentMode::AutoNoVsync),
                ..surface
            },
            "describes() is deliberately weaker than equality, not a \
             replacement for it"
        );
    }

    #[test]
    fn output_validity_tracks_pending_and_completion() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
        let mut driver = WindowDriver::builder(WindowToken(1), &shared, true).build();
        assert!(!driver.output_valid, "first frame has no presented output");

        driver.output_valid = true;
        let paint = driver.finish_cpu_frame(
            &mut frontend,
            report(Some(RenderPlan {
                clear: Color::BLACK,
                damage: Damage::Full,
            })),
        );
        assert!(matches!(paint.mode, PresentMode::Direct(_)));
        assert!(
            !driver.output_valid,
            "paint stays pending until acquire and submit complete"
        );

        driver.output_valid = true;
        assert!(driver.output_valid, "successful submit restores validity");

        let skip = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip.mode, PresentMode::SkipNoop));
        assert!(
            driver.output_valid,
            "SkipNoop preserves valid target pixels"
        );

        driver.strategy = PresentStrategy::BackbufferCopy;
        let skip_copy = driver.finish_cpu_frame(&mut frontend, report(None));
        assert!(matches!(skip_copy.mode, PresentMode::SkipCopy));
        assert!(
            !driver.output_valid,
            "SkipCopy stays pending until the copy is submitted"
        );
        driver.output_valid = true;
        assert!(driver.output_valid, "successful copy restores validity");
    }
}

/// What a driver owns for as long as it exists: its place in the
/// app-global window directory, and a render-owner id no sibling shares.
mod lifecycle_tests {
    use crate::host::shared::HostShared;
    use crate::host::window_driver::WindowDriver;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;
    use crate::window::window_token::WindowToken;

    /// The directory entry belongs to the driver, not to whoever built or
    /// closed it: `build` mints it and `Drop` retires it.
    ///
    /// Both halves matter. A registration made when the builder is created
    /// would leave a token live for the rest of the session whenever a
    /// builder is dropped unbuilt, with `Ui::window_open` answering true
    /// for a window that never opened. A retirement left to the host would
    /// have to be remembered on two different close paths.
    #[test]
    fn a_driver_owns_its_directory_entry_from_build_to_drop() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let token = WindowToken(11);

        let builder = WindowDriver::builder(token, &shared, true);
        drop(builder);
        assert!(
            !shared.resources.windows.contains(token),
            "a builder that never built owns no window",
        );

        let driver = WindowDriver::builder(token, &shared, true).build();
        assert!(shared.resources.windows.contains(token));
        drop(driver);
        assert!(
            !shared.resources.windows.contains(token),
            "the entry cannot outlive the driver",
        );
    }

    #[test]
    fn window_drivers_have_distinct_render_owners() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let first = WindowDriver::builder(WindowToken(1), &shared, true).build();
        let second = WindowDriver::builder(WindowToken(2), &shared, true).build();

        assert_ne!(first.render_owner, second.render_owner);
    }
}

mod record_store_tests {
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::app::App;
    use crate::app::internals::RecordApp;
    use crate::host::clock::FixedClock;
    use crate::host::shared::HostShared;
    use crate::host::window_driver::{PresentStrategy, WindowDriver};
    use crate::primitives::color::{Color, ColorU8};
    use crate::primitives::mesh::{Mesh, MeshVertex};
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::shape::Shape;
    use crate::shape::polyline::PolylineColors;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::ui::frame_report::FrameProcessing;
    use crate::widgets::panel::Panel;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;
    use crate::{Configure, Display, WindowToken};

    #[derive(Debug, PartialEq)]
    struct RecordPayloadSnapshot {
        mesh_vertices: Vec<MeshVertex>,
        mesh_indices: Vec<u32>,
        polyline_points: Vec<Vec2>,
        polyline_colors: Vec<ColorU8>,
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
            mesh_indices: store.meshes.indices.clone(),
            polyline_points: store.polyline_points.clone(),
            polyline_colors: store.polyline_colors.clone(),
            text: store.interned_text().all().to_owned(),
        }
    }

    fn record_scene(
        ui: &mut Ui,
        mesh: &Mesh,
        points: &[Vec2],
        colors: &[Color],
        label: &str,
        id: &'static str,
    ) {
        Panel::zstack()
            .id(WidgetId::from_hash(id))
            .size(96.0)
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(mesh));
                ui.add_shape(Shape::polyline(
                    points,
                    PolylineColors::PerPoint(colors),
                    3.0,
                ));
                let label = ui.intern(label);
                Text::new(label)
                    .id(WidgetId::from_hash((id, "text")))
                    .show(ui);
                Spinner::new()
                    .id(WidgetId::from_hash((id, "spinner")))
                    .diameter(92.0)
                    .show(ui);
            });
    }

    #[test]
    fn cpu_frame_forwards_token_through_app_lifecycle() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
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
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let mut frontend = Frontend::new(8192, shared.gradient_atlas.clone());
        let mut window_a = WindowDriver::builder(WindowToken(1), &shared, true)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let mut window_b = WindowDriver::builder(WindowToken(2), &shared, true)
            .clock(Box::new(FixedClock::new(Duration::ZERO)))
            .build();
        let display = Display::from_physical(UVec2::new(112, 112), 1.0);

        let mesh_a = Mesh::filled_triangle(
            Vec2::new(12.0, 14.0),
            Vec2::new(72.0, 20.0),
            Vec2::new(26.0, 74.0),
            Color::rgb(0.15, 0.65, 0.95),
        );
        let points_a = [
            Vec2::new(8.0, 82.0),
            Vec2::new(28.0, 10.0),
            Vec2::new(68.0, 84.0),
            Vec2::new(88.0, 12.0),
        ];
        let colors_a = [
            Color::rgb(1.0, 0.0, 0.0),
            Color::WHITE,
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(0.0, 0.0, 1.0),
        ];

        let mesh_b = Mesh::filled_polygon(
            &[
                Vec2::new(78.0, 8.0),
                Vec2::new(90.0, 46.0),
                Vec2::new(58.0, 88.0),
                Vec2::new(14.0, 70.0),
                Vec2::new(8.0, 24.0),
            ],
            Color::rgb(0.9, 0.2, 0.65),
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
            Color::WHITE,
            Color::rgb(0.0, 0.0, 1.0),
            Color::rgb(0.0, 1.0, 0.0),
            Color::rgb(1.0, 0.0, 0.0),
            Color::BLACK,
            Color::WHITE,
        ];

        let mut app_a = RecordApp::new(|ui| {
            record_scene(ui, &mesh_a, &points_a, &colors_a, "retained A", "window-a");
        });
        let _ = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        window_a.output_valid = true;
        let retained = snapshot(&window_a);
        assert_eq!(retained.mesh_vertices.len(), 3);
        assert_eq!(retained.polyline_points.len(), 4);
        assert_eq!(retained.text, "retained A");

        let mut app_b = RecordApp::new(|ui| {
            record_scene(
                ui,
                &mesh_b,
                &points_b,
                &colors_b,
                "window B has a much longer label",
                "window-b",
            );
        });
        let _ = window_b.cpu_frame(&mut frontend, display, &mut app_b);
        window_b.output_valid = true;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.cpu_frame(&mut frontend, display, &mut app_a);
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}
