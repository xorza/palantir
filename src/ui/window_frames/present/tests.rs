//! Present tests, grouped by the decision each pins: present-mode selection, output
//! validity, record-store lifecycle.

use crate::display::frame_stamp::FrameStamp;
use crate::display::system_scale::SystemScale;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
use crate::gpu::render_owner_id::RenderOwners;
use crate::gpu::texel::TargetFormat;
use crate::renderer::frontend::Frontend;
use crate::ui::app::App;
use crate::ui::frame_report::FrameReport;
use crate::ui::resources::UiResources;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::{
    BuildCtx, CpuFrame, FrameSpec, PresentStrategy, TargetKey, WindowSpec,
};
use crate::window::window_token::WindowToken;
use glam::UVec2;

/// What a host sets on a window it opens: its strategy, and the host's pixel snapping, which
/// the shared settings hold.
#[derive(Clone, Copy, Debug)]
struct FramesConfig {
    strategy: PresentStrategy,
    pixel_snap: bool,
}

const fn config(strategy: PresentStrategy, pixel_snap: bool) -> FramesConfig {
    FramesConfig {
        strategy,
        pixel_snap,
    }
}

/// The largest texture edge of the device a CPU-only frame plans its cutouts for.
const DEVICE_EDGE: u32 = 8192;

/// The frames of `token` over `shared`, set by `config`, on a stream with no device behind it that
/// shades every cutout.
fn new_frames(token: WindowToken, shared: &UiResources, config: FramesConfig) -> WindowFrames {
    shared.settings().set_pixel_snap(config.pixel_snap);
    let spec = WindowSpec {
        token,
        owner: RenderOwners::default().reserve(),
        strategy: config.strategy,
    };
    let mut frames = WindowFrames::new(spec, shared);
    frames.disable_cutout_tables();
    frames
}

impl WindowFrames {
    /// [`Self::build_paint`] on placements of its own, as a test with no held cutouts builds.
    fn build_alone<'f>(&mut self, frontend: &'f mut Frontend, report: FrameReport) -> CpuFrame<'f> {
        let ctx = BuildCtx {
            frontend,
            placing: CutoutPlacing {
                placements: &mut CutoutPlacements::default(),
                pool: &mut AtlasPool::new(DEVICE_EDGE),
            },
        };
        self.build_paint(ctx, report)
    }

    /// A painted frame's CPU half under `stamp`, with no target noted and no device.
    fn paint_at<'f, T: App>(
        &mut self,
        frontend: &'f mut Frontend,
        stamp: FrameStamp,
        app: &mut T,
    ) -> CpuFrame<'f> {
        let report = self.frame(stamp, app);
        self.build_alone(frontend, report)
    }
}

/// A texture frame of `physical` at `system_scale`, at time zero.
const fn spec(physical: UVec2, system_scale: f32) -> FrameSpec {
    FrameSpec {
        system_scale: SystemScale::new(system_scale).unwrap(),
        ..FrameSpec::at_rest(TargetKey {
            physical,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: true,
        })
    }
}

mod present_mode_tests {
    use crate::damage::Damage;
    use crate::damage::region::{DamageRegion, MergePolicy};
    use crate::damage::sweep::policy::{Decision, FrameFacts, Policy};
    use crate::damage::sweep::scenario::catalog;
    use crate::damage::sweep::stage::Stage;
    use crate::gpu::frame::submission::Route;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::window_frames::present::PresentStrategy;
    use crate::ui::window_frames::present::PresentStrategy::{
        BackbufferCopy, DirectAdaptive, Retained,
    };
    use crate::ui::window_frames::present::present_facts::{
        PaintPath, PresentFacts, promote_share, rows_per_mpx,
    };
    use glam::UVec2;

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
            MergePolicy::DEFAULT,
            SURFACE,
        );
        RenderPlan {
            clear: RgbaF32::BLACK,
            damage: Damage::Partial(damage),
        }
    }
    /// The path of `plan` on a scene whose promote share is 0.4.
    fn path(plan: RenderPlan, strategy: PresentStrategy, fresh: bool) -> PaintPath {
        PresentFacts {
            strategy,
            capturing: false,
            backbuffer_fresh: fresh,
            promote_share: Some(0.4),
        }
        .path(plan)
    }

    /// `d / (d + 400)`: 400 rows on a million pixels read one half, 1,200 three quarters, none
    /// nothing. Twice the rows on twice the pixels is the same density. Backdrops over all of the
    /// surface count for 300 rows on a million pixels: 300 / 700 alone, 700 / 1,100 beside 400
    /// rows, and half the surface half of that.
    #[test]
    fn the_promote_share_follows_the_scenes_density() {
        let mpx = UVec2::new(1000, 1000);
        assert_eq!(promote_share(400, 0.0, mpx), 0.5);
        assert_eq!(promote_share(1200, 0.0, mpx), 0.75);
        assert_eq!(promote_share(0, 0.0, mpx), 0.0);
        assert_eq!(promote_share(800, 0.0, UVec2::new(2000, 1000)), 0.5);
        assert_eq!(promote_share(0, 1.0, mpx), 300.0 / 700.0);
        assert_eq!(promote_share(400, 1.0, mpx), 700.0 / 1100.0);
        assert_eq!(promote_share(0, 0.5, mpx), 150.0 / 550.0);
    }

    const DIRECT_FULL: PaintPath = PaintPath::direct(RenderPlan {
        clear: RgbaF32::BLACK,
        damage: Damage::Full,
    });

    /// A frame that takes a screenshot paints all of the backbuffer, whatever its damage, the
    /// strategy or the backbuffer's freshness, as the capture reads it.
    #[test]
    fn a_capturing_frame_paints_the_whole_backbuffer() {
        for strategy in [DirectAdaptive, BackbufferCopy, Retained] {
            for fresh in [false, true] {
                let facts = PresentFacts {
                    strategy,
                    capturing: true,
                    backbuffer_fresh: fresh,
                    promote_share: None,
                };
                assert_eq!(facts.path(full()), PaintPath::through_backbuffer(full()));
                assert_eq!(
                    facts.path(partial(10.0, 10.0)),
                    PaintPath::through_backbuffer(full())
                );
            }
        }
    }

    /// A sweep's production policy decides each scenario as the frame path does: the damage's
    /// classification, then the desktop's present path, on a backbuffer that holds the last frame,
    /// on a thin screen and on one so dense its share passes the full-repaint threshold.
    #[test]
    fn the_sweeps_production_policy_is_the_frame_paths() {
        let stage = Stage::SCENE;
        let surface = stage.surface();
        for (row_count, backdrop_share) in [(40_usize, 0.0), (400, 0.0), (400, 0.6), (40_000, 0.0)]
        {
            let promote_share = promote_share(row_count, backdrop_share, stage.physical);
            let facts = FrameFacts {
                surface,
                rows_per_mpx: rows_per_mpx(row_count, stage.physical),
                backdrop_share,
            };
            for scenario in catalog() {
                for raw in stage.raw_frames(&scenario) {
                    let collapsed =
                        DamageRegion::collapse_from(&raw, MergePolicy::DEFAULT, surface);
                    let damage = Damage::new(collapsed).expect("a scenario damages something");
                    let plan = RenderPlan {
                        clear: RgbaF32::BLACK,
                        damage,
                    };
                    let path = PresentFacts {
                        strategy: DirectAdaptive,
                        capturing: false,
                        backbuffer_fresh: true,
                        promote_share: Some(promote_share),
                    }
                    .path(plan);
                    let real = match (path.route, path.plan.damage) {
                        (Route::Direct, Damage::Full) => Decision::Full,
                        (Route::ViaBackbuffer, Damage::Partial(damage)) => {
                            Decision::partial(damage.region.iter_rects().collect())
                        }
                        other => panic!("{}: {other:?}", scenario.name),
                    };
                    assert_eq!(
                        Policy::PRODUCTION.decide(&raw, &facts),
                        real,
                        "{} at {row_count} rows",
                        scenario.name
                    );
                }
            }
        }
    }

    #[test]
    fn backbuffer_copy_fills_target_through_backbuffer() {
        // Fresh target each call: paint via the backbuffer (Full or Partial).
        for fresh in [false, true] {
            assert_eq!(
                path(full(), BackbufferCopy, fresh),
                PaintPath::through_backbuffer(full())
            );
            assert_eq!(
                path(partial(10.0, 10.0), BackbufferCopy, fresh),
                PaintPath::through_backbuffer(partial(10.0, 10.0))
            );
        }
    }

    /// A retained target keeps its pixels, so every painted frame paints into it as planned,
    /// however much it covers and whatever the backbuffer holds.
    #[test]
    fn retained_paints_every_frame_into_the_target() {
        for fresh in [false, true] {
            for plan in [full(), partial(10.0, 10.0), partial(90.0, 90.0)] {
                assert_eq!(path(plan, Retained, fresh), PaintPath::direct(plan));
            }
        }
    }

    #[test]
    fn direct_adaptive_paints_a_whole_frame_direct() {
        for fresh in [false, true] {
            assert_eq!(
                path(full(), DirectAdaptive, fresh),
                PaintPath::direct(full())
            );
        }
    }

    #[test]
    fn direct_adaptive_small_partial_tracks_backbuffer_freshness() {
        let small = partial(10.0, 10.0);
        assert_eq!(
            path(small, DirectAdaptive, true),
            PaintPath::through_backbuffer(small)
        );
        assert_eq!(
            path(small, DirectAdaptive, false),
            PaintPath::through_backbuffer(full())
        );
    }

    #[test]
    fn direct_adaptive_large_partial_promotes_to_direct() {
        let large = partial(80.0, 80.0);
        for fresh in [false, true] {
            assert_eq!(path(large, DirectAdaptive, fresh), DIRECT_FULL);
        }
    }

    #[test]
    fn direct_adaptive_promote_threshold_is_strict() {
        // At or below 0.4 stays on the backbuffer path (`>`, not `>=`); just over
        // promotes. 63×63 (0.3969) and 64×64 (0.4096) straddle the line, and 40×100
        // = 4_000 sits on it as `0.4f32`, which only a strict compare keeps.
        for (w, h) in [(63.0, 63.0), (40.0, 100.0)] {
            assert_eq!(
                path(partial(w, h), DirectAdaptive, true).route,
                Route::ViaBackbuffer,
                "{w}×{h}",
            );
        }
        assert_eq!(path(partial(64.0, 64.0), DirectAdaptive, true), DIRECT_FULL);
    }
}

mod output_validity_tests {
    use crate::display::system_scale::SystemScale;
    use crate::ui::window_frames::present::TargetKey;
    use crate::ui::window_frames::present::tests::new_frames;
    use glam::UVec2;

    use crate::gpu::frame::submission::Route;
    use crate::gpu::surface::capture::Capture;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::texel::TargetFormat;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::color::RgbaF32;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_plan::RenderPlan;
    use crate::ui::window_frames::WindowFrames;
    use crate::ui::window_frames::present::screenshot_lane::PendingScreenshot;
    use crate::ui::window_frames::present::tests::{config, spec};
    use crate::ui::window_frames::present::{BuiltFrame, FrameSpec, PresentStrategy};
    use crate::window::screenshot::{ScreenshotId, ScreenshotRequest};
    use std::time::Duration;

    use crate::damage::{Damage, DamageBase};

    use crate::ui::Ui;
    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use crate::ui::resources::UiResources;
    use crate::window::vsync::Vsync;
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_token::WindowToken;

    fn frames(token: WindowToken, shared: &UiResources) -> WindowFrames {
        new_frames(token, shared, config(PresentStrategy::DirectAdaptive, true))
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
        let mut frames = frames(token, &shared);
        let opened = WindowToken(18);
        let mut commands = WindowCommands::default();

        frames
            .ui
            .open_window(opened, WindowConfig::new("inspector"));
        frames.ui.request_cursor(CursorIcon::Pointer);
        frames.ui.window_frame_mut().facts.close_requested = true;

        let output = frames.drain_window_output(&mut commands);
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
        assert!(frames.ui.window_requests().commands.seen().is_empty());
        // Drained by `append`, not `mem::take`, so the recorder keeps its buffer.
        let capacity = frames.ui.window_requests().commands.capacity();
        assert!(capacity > 0);

        frames.ui.window_frame_mut().facts.close_requested = true;
        frames.ui.keep_open();
        let mut vetoed = WindowCommands::default();
        frames.drain_window_output(&mut vetoed);
        assert!(vetoed.seen().is_empty());

        let mut settled = WindowCommands::default();
        frames.drain_window_output(&mut settled);
        assert!(settled.seen().is_empty());
        assert!(!frames.ui.window_requests().close_vetoed);
        assert_eq!(
            frames.ui.window_requests().commands.capacity(),
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
        let mut frames = frames(WindowToken(3), &shared);
        let mut commands = WindowCommands::default();

        assert_eq!(frames.ui.vsync(), Vsync::On, "vsync is on unless asked off");
        assert_eq!(frames.drain_window_output(&mut commands).vsync, Vsync::On);

        frames.ui.set_vsync(Vsync::Off);
        assert_eq!(frames.ui.vsync(), Vsync::Off, "the setter reads back");
        assert_eq!(frames.drain_window_output(&mut commands).vsync, Vsync::Off);
        assert_eq!(
            frames.drain_window_output(&mut commands).vsync,
            Vsync::Off,
            "the level survives the drain that delivered it",
        );

        frames.ui.set_vsync(Vsync::On);
        frames.ui.set_vsync(Vsync::Off);
        assert_eq!(frames.drain_window_output(&mut commands).vsync, Vsync::Off);
    }

    fn report(plan: Option<RenderPlan>) -> FrameReport {
        FrameReport {
            repaint_requested: false,
            repaint_at: None,
            earliest_repaint_at: None,
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
        let mut frames = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let first = spec(UVec2::new(64, 48), 1.0);
        let resized = FrameSpec {
            target: TargetKey {
                physical: UVec2::new(65, 48),
                ..first.target
            },
            ..first
        };
        let reformatted = FrameSpec {
            target: TargetKey {
                format: TargetFormat::new(wgpu::TextureFormat::Bgra8UnormSrgb),
                ..resized.target
            },
            ..resized
        };
        let uncopyable = FrameSpec {
            target: TargetKey {
                gives_copy: false,
                ..reformatted.target
            },
            ..reformatted
        };
        let rescaled = FrameSpec {
            system_scale: SystemScale::new(2.0).unwrap(),
            refresh_millihertz: Some(120_000),
            now: Duration::from_secs(1),
            ..uncopyable
        };
        // Whether noting `spec` cleared what a kept target holds.
        let mut clears = |spec: &FrameSpec| {
            frames.damage_base = DamageBase::Prev;
            frames.stream.assume_backbuffer_fresh();
            frames.note_target(spec);
            assert_eq!(frames.output_valid(), frames.stream.is_backbuffer_fresh());
            !frames.output_valid()
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

    /// A frame below paint has no target to lose its baseline on and no device to take its
    /// screenshots, so its close keeps the base and drops each ask.
    #[test]
    fn a_frame_below_paint_keeps_its_base_and_drops_its_screenshots() {
        let shared = UiResources::isolated_mono();
        let mut frames = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::BackbufferCopy, true),
        );
        let mut asked = None;
        let mut app = |ui: &mut Ui| asked = ui.request_screenshot(None);
        let _ = frames.record(spec(UVec2::new(100, 80), 1.0), &mut app);
        assert!(asked.is_some());
        assert!(!frames.output_valid(), "a first target discards the base");
        assert_eq!(frames.ui.window_requests().screenshots.len(), 1);

        frames.finish_unpainted();

        assert!(frames.output_valid());
        assert_eq!(frames.ui.window_requests().screenshots.len(), 0);
        assert_eq!(frames.screenshots.requests.len(), 0);
    }

    /// A built frame dropped unsubmitted, as a failed acquire or a lost device leaves it, leaves
    /// the base discarded, so the next frame of the same scene paints whole; one that completed
    /// paints nothing.
    #[test]
    fn a_frame_dropped_unsubmitted_makes_the_next_frame_whole() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        for (submitted, next_paints) in [(false, true), (true, false)] {
            let mut frames = new_frames(
                WindowToken(1),
                &shared,
                config(PresentStrategy::DirectAdaptive, true),
            );
            let mut app = |_: &mut Ui| {};
            let at = spec(UVec2::new(100, 80), 1.0);
            let first = frames.record(at, &mut app);
            let _ = frames.build_alone(&mut frontend, first);
            if submitted {
                // What the GPU half does, which `offscreen::tests` runs on a device.
                frames.damage_base = DamageBase::Prev;
            }
            let next = frames.record(at, &mut app);
            assert_eq!(next.plan.is_some(), next_paints, "submitted: {submitted}");
            if next_paints {
                assert_eq!(next.plan.map(|plan| plan.damage), Some(Damage::Full));
            }
        }
    }

    #[test]
    fn output_validity_tracks_pending_and_completion() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        let mut frames = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        assert!(
            !frames.output_valid(),
            "first frame has no presented output"
        );

        frames.damage_base = DamageBase::Prev;
        let paint = frames.build_alone(
            &mut frontend,
            report(Some(RenderPlan {
                clear: RgbaF32::BLACK,
                damage: Damage::Full,
            })),
        );
        assert_eq!(
            paint.built.path().map(|path| path.route),
            Some(Route::Direct)
        );
        assert!(
            !frames.output_valid(),
            "paint stays pending until acquire and submit complete"
        );

        // The GPU half completes the paint (`offscreen::tests` runs it); a window
        // without one has no submit, so a still frame's precondition is set here.
        frames.damage_base = DamageBase::Prev;

        let skip = frames.build_alone(&mut frontend, report(None));
        assert!(matches!(skip.built, BuiltFrame::Still));
        assert!(
            frames.output_valid(),
            "a still frame preserves valid target pixels"
        );
        assert!(
            !skip.report.repaint_requested,
            "a still frame asks for nothing"
        );

        // A screenshot still on the device keeps the frames coming, as nothing else wakes the
        // host when it lands; a still frame asks for nothing once it has.
        frames.screenshots.pending.push_back(PendingScreenshot {
            capture: Capture::never_landing(&headless_test_gpu()),
            request: ScreenshotRequest {
                region: Rect::new(0.0, 0.0, 1.0, 1.0),
                id: ScreenshotId::FIRST,
            },
            scale: 1.0,
        });
        let waiting = frames.build_alone(&mut frontend, report(None));
        assert!(matches!(waiting.built, BuiltFrame::Still));
        assert!(
            waiting.report.repaint_requested,
            "a pending screenshot asks for the next frame"
        );
        frames.screenshots.pending.clear();
        let landed = frames.build_alone(&mut frontend, report(None));
        assert!(!landed.report.repaint_requested);

        frames.strategy = PresentStrategy::BackbufferCopy;
        let skip_copy = frames.build_alone(&mut frontend, report(None));
        assert!(matches!(skip_copy.built, BuiltFrame::CopyBackbuffer));
        assert!(
            !frames.output_valid(),
            "a copy stays pending until it is submitted"
        );
    }
}

/// What a window's frames own while they exist: its place in the app-global window directory
/// and a render-owner id no sibling shares.
mod lifecycle_tests {
    use crate::ui::resources::UiResources;
    use crate::ui::window_frames::present::PresentStrategy;
    use crate::ui::window_frames::present::tests::config;
    use crate::ui::window_frames::present::tests::new_frames;
    use crate::window::window_token::WindowToken;

    /// The directory entry belongs to the frames, not whoever opened or closed it:
    /// `new` mints it and `Drop` retires it, as a retirement left to the host would
    /// need remembering on two close paths.
    #[test]
    fn a_window_owns_its_directory_entry_from_build_to_drop() {
        let shared = UiResources::isolated_mono();
        let token = WindowToken(11);
        assert!(!shared.windows().contains(token));

        let frames = new_frames(
            token,
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        assert!(shared.windows().contains(token));
        drop(frames);
        assert!(
            !shared.windows().contains(token),
            "the entry cannot outlive the frames",
        );
    }
}

mod record_store_tests {
    use crate::damage::DamageBase;
    use crate::ui::window_frames::present::tests::new_frames;
    use std::time::Duration;

    use glam::{UVec2, Vec2};

    use crate::ui::app::App;

    use crate::display::frame_stamp::FrameStamp;
    use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::frontend::Frontend;
    use crate::ui::window_frames::WindowFrames;
    use crate::ui::window_frames::present::PresentStrategy;
    use crate::ui::window_frames::present::tests::config;

    use crate::shape::Shape;

    use crate::display::Display;
    use crate::primitives::paint::stroke::Stroke;
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
        records: Vec<WindowToken>,
    }

    impl App for LifecycleApp {
        fn record(&mut self, win: WindowToken, _ui: &mut Ui) {
            self.records.push(win);
        }
    }

    fn snapshot(frames: &WindowFrames) -> RecordPayloadSnapshot {
        let store = frames.ui.record_store();
        RecordPayloadSnapshot {
            mesh_vertices: store.mesh_vertices.clone(),
            mesh_idxs: store.mesh_idxs.clone(),
            polyline_points: store.polyline_points.clone(),
            polyline_colors: store.polyline_colors.clone(),
            text: store.text.interned().all().to_owned(),
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
                    ui.add_shape(Shape::mesh(self.mesh), RgbaF32::WHITE);
                    ui.add_shape(
                        Shape::polyline(self.points).per_point(self.colors),
                        Stroke::new(RgbaF32::WHITE, 3.0),
                    );
                    let label = ui.intern(self.label);
                    Text::new(label)
                        .id(WidgetId::from_hash((id, "text")))
                        .show(ui);
                    Spinner::new()
                        .id(WidgetId::from_hash((id, "spinner")))
                        .size(92.0)
                        .show(ui);
                });
        }
    }

    #[test]
    fn a_frame_forwards_token_through_app_lifecycle() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        let token = WindowToken(17);
        let mut window = new_frames(
            token,
            &shared,
            config(PresentStrategy::DirectAdaptive, false),
        );
        assert_eq!(window.strategy, PresentStrategy::DirectAdaptive);
        assert!(!window.ui.resources.settings().pixel_snap());
        let mut app = LifecycleApp::default();

        let _ = window.paint_at(
            &mut frontend,
            FrameStamp::new(
                Display::from_physical(UVec2::new(112, 112), 1.0),
                Duration::ZERO,
            ),
            &mut app,
        );

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
        let mut frontend = Frontend::for_test();
        let mut window_a = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let mut window_b = new_frames(
            WindowToken(2),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let display = FrameStamp::new(
            Display::from_physical(UVec2::new(112, 112), 1.0),
            Duration::ZERO,
        );

        let mesh_a = Mesh::filled_polygon(
            &[
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
        let mut app_a = |ui: &mut Ui| scene_a.record(ui);
        let _ = window_a.paint_at(&mut frontend, display, &mut app_a);
        window_a.damage_base = DamageBase::Prev;
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
        let mut app_b = |ui: &mut Ui| scene_b.record(ui);
        let _ = window_b.paint_at(&mut frontend, display, &mut app_b);
        window_b.damage_base = DamageBase::Prev;
        assert_eq!(snapshot(&window_a), retained);

        let paint_only = window_a.paint_at(&mut frontend, display, &mut app_a);
        assert_eq!(paint_only.report.processing, FrameProcessing::PaintOnly);
        assert_eq!(snapshot(&window_a), retained);
    }
}

/// The one place both host-owned display settings reach a frame.
mod display_tests {
    use crate::display::system_scale::SystemScale;
    use crate::ui::window_frames::present::tests::new_frames;
    use glam::UVec2;

    use crate::display::user_scale::UserScale;
    use crate::primitives::geometry::size::Size;
    use crate::ui::window_frames::present::PresentStrategy;
    use crate::ui::window_frames::present::tests::{config, spec};

    use crate::ui::resources::UiResources;
    use crate::window::window_token::WindowToken;

    /// A window mints its stamp from what it owns, so a scale the app wrote
    /// during a frame is in the very next one. `pixel_snap` rides the same call.
    #[test]
    fn the_mint_folds_in_the_app_scale_and_the_hosts_snap() {
        let shared = UiResources::isolated_mono();
        let mut frames = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, false),
        );

        let plain = frames.stamp(&spec(UVec2::new(800, 600), 2.0)).display;
        assert_eq!(plain.system_scale, SystemScale::new(2.0).unwrap());
        assert_eq!(plain.user_scale, UserScale::ONE);
        assert_eq!(plain.scale_factor(), 2.0);
        assert!(!plain.pixel_snap, "the snap comes from the host, not here");

        frames.ui.set_user_scale(UserScale::new(1.25).unwrap());
        let zoomed = frames.stamp(&spec(UVec2::new(800, 600), 2.0)).display;
        assert_eq!(
            zoomed.system_scale,
            SystemScale::new(2.0).unwrap(),
            "the platform's half is untouched"
        );
        assert_eq!(zoomed.user_scale, UserScale::new(1.25).unwrap());
        assert_eq!(zoomed.scale_factor(), 2.5);
        assert_eq!(zoomed.logical_size(), Size::new(320.0, 240.0));
        assert_eq!(zoomed.system_logical_size(), Size::new(400.0, 300.0));
    }

    /// The scale is app-global, so two windows over one `UiResources` mint the same
    /// one.
    #[test]
    fn two_windows_mint_the_one_scale() {
        let shared = UiResources::isolated_mono();
        let mut first = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let second = new_frames(
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

    use crate::damage::{Damage, DamageBase};
    use crate::display::Display;
    use crate::display::frame_stamp::FrameStamp;
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::frame::submission::Route;
    use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
    use crate::gpu::render_owner_id::RenderOwners;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::shadow::Shadow;
    use crate::renderer::frontend::Frontend;
    use crate::shape::Shape;
    use crate::ui::Ui;
    use crate::ui::resources::UiResources;
    use crate::ui::window_frames::WindowFrames;
    use crate::ui::window_frames::present::{BuildCtx, PresentStrategy, WindowSpec};
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
        let mut frontend = Frontend::for_test();
        let spec = WindowSpec {
            token: WindowToken(1),
            owner: RenderOwners::default().reserve(),
            strategy: PresentStrategy::BackbufferCopy,
        };
        let mut frames = WindowFrames::new(spec, &shared);
        frames.attach(RenderStream::detached(frames.render_owner()));
        let display = FrameStamp::new(
            Display::from_physical(UVec2::new(600, 200), 1.0),
            Duration::ZERO,
        );
        let cards = Cell::new(3);
        let tick = Cell::new(false);
        let mut app = |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("cards")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for card in 0..cards.get() {
                        ui.add_shape(
                            Shape::rect()
                                .at(Rect::new(10.0 + 70.0 * card as f32, 40.0, 40.0, 30.0))
                                .corners(8.0),
                            Shadow {
                                color: RgbaF32::BLACK,
                                blur: 4.0,
                                ..Shadow::NONE
                            },
                        );
                    }
                    let fill = if tick.get() {
                        RgbaF32::WHITE
                    } else {
                        RgbaF32::BLACK
                    };
                    Block::new()
                        .id_salt("tick")
                        .at((500.0, 150.0))
                        .size(10.0)
                        .chrome(Chrome::new(fill))
                        .show(ui);
                });
        };
        let mut placements = CutoutPlacements::default();
        let mut pool = AtlasPool::new(super::DEVICE_EDGE);
        let mut damage = |cards_now: usize, tick_now: bool| {
            cards.set(cards_now);
            tick.set(tick_now);
            let report = frames.frame(display, &mut app);
            let ctx = BuildCtx {
                frontend: &mut frontend,
                placing: CutoutPlacing {
                    placements: &mut placements,
                    pool: &mut pool,
                },
            };
            let frame = frames.build_paint(ctx, report);
            frames.damage_base = DamageBase::Prev;
            let path = frame.built.path().expect("a painting frame");
            assert_eq!(path.route, Route::ViaBackbuffer);
            path.plan.damage
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
    use crate::display::system_scale::SystemScale;
    use std::cell::Cell;

    use glam::{UVec2, Vec2};
    use image::imageops;

    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::texel::TargetFormat;
    use crate::host::harness::Harness;
    use crate::host::harness::harness_builder::Depth;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::geometry::size::Size;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
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
        window: Harness,
        /// The id of the last ask, whose image a frame looks for.
        asked: Option<ScreenshotId>,
    }

    impl Scene {
        fn new(format: wgpu::TextureFormat, scale: f32) -> Self {
            let gpu = headless_test_gpu();
            let window = Harness::builder(PHYSICAL)
                .system_scale(SystemScale::new(scale).unwrap())
                .mono()
                .depth(Depth::Gpu {
                    gpu: gpu.handles(),
                    format: TargetFormat::new(format),
                })
                .build();
            Self {
                window,
                asked: None,
            }
        }

        /// The image of the last ask, in the frame it lands in.
        fn landed(&self) -> Option<&Screenshot> {
            self.window.ui().screenshot_taken(self.asked?)
        }

        /// One frame.
        fn frame(&mut self, frame: Frame) -> FrameRun {
            let asked = Cell::new(self.asked);
            let seen = Cell::new(None);
            let mut app = |ui: &mut Ui| {
                if let Some(shot) = asked.get().and_then(|id| ui.screenshot_taken(id)) {
                    seen.set(Some(shot.clone()));
                }
                Block::new()
                    .id_salt("red")
                    .at((2.0, 2.0))
                    .size(10.0)
                    .chrome(Chrome::new(RgbaF32::hex(0xd23f7a)))
                    .show(ui);
                Block::new()
                    .id_salt("blue")
                    .at((14.0, 6.0))
                    .size(8.0)
                    .chrome(Chrome::new(RgbaF32::hex(0x2f6fd0)))
                    .show(ui);
                if let Frame::Asking(region) = frame {
                    asked.set(ui.request_screenshot(region));
                }
            };
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
        let mut others = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba16Float,
        ]
        .map(|format| (format, Scene::new(format, 2.0)));
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
            for (format, other) in &mut others {
                let shot = other.capture(Some(region));
                assert_eq!(shot.image(), image, "{format:?} {region:?}");
            }
        }
        let empty = Rect {
            min: Vec2::new(4.0, 4.0),
            size: Size::new(0.0, 3.0),
        };
        assert_eq!(
            scene.window.ui_mut().request_screenshot(Some(empty)),
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
        let mut app = |ui: &mut Ui| {
            let mut ask = |region| {
                ui.request_screenshot(Some(region))
                    .expect("a region with pixels")
            };
            let first = ask(left);
            let again = ask(left);
            let second = ask(right);
            ids.set(Some([first, again, second]));
        };
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

/// Each refusal of a partial frame, from the plain facts the decision reads. A stale census needs
/// a device's tables, and `cutout_census_tests` holds it.
mod escalation_tests {
    use crate::damage::Damage;
    use crate::damage::region::{DamageRegion, MergePolicy};
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::frame::submission::Route;
    use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::identity::backdrop_key::BackdropKey;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::material::Material;
    use crate::renderer::frontend::Frontend;
    use crate::renderer::render_buffer::backdrop_row::BackdropSource;
    use crate::renderer::render_plan::RenderPlan;
    use crate::shape::Shape;
    use crate::ui::Ui;
    use crate::ui::resources::UiResources;
    use crate::ui::window_frames::present::PresentStrategy::{
        BackbufferCopy, DirectAdaptive, Retained,
    };
    use crate::ui::window_frames::present::present_facts::{PaintPath, PresentFacts};
    use crate::ui::window_frames::present::tests::{config, new_frames, spec};
    use crate::ui::window_frames::present::{BuildCtx, FrameSpec, PresentStrategy, TargetKey};
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    const SIDE: f32 = 400.0;
    const PHYSICAL: UVec2 = UVec2::splat(400);

    /// One frame's facts. Backdrops are bars the full height of the surface, 8 wide and 40 apart.
    #[derive(Clone, Copy, Debug)]
    struct Row {
        what: &'static str,
        strategy: PresentStrategy,
        /// Bands the full width of the surface, 10 high and 80 apart.
        bands: u32,
        backdrops: u32,
        outlined: bool,
        gives_copy: bool,
        fresh: bool,
        through_backbuffer: bool,
        whole: bool,
    }

    /// A partial row of a retained target that gives a copy, and whose backbuffer is stale, that
    /// paints partial and direct.
    const KEPT: Row = Row {
        what: "",
        strategy: Retained,
        bands: 1,
        backdrops: 0,
        outlined: false,
        gives_copy: true,
        fresh: false,
        through_backbuffer: false,
        whole: false,
    };

    /// `count` backdrops, each a bar as [`Row`] says.
    fn bars(ui: &mut Ui, count: u32) {
        Panel::canvas()
            .id_salt("backdrops")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for i in 0..count {
                    ui.add_shape(
                        Shape::rect().at(Rect::new(20.0 + 40.0 * i as f32, 0.0, 8.0, SIDE)),
                        Material::blur(2.0),
                    );
                }
            });
    }

    /// A partial plan of `count` bands, each as [`Row`] says.
    fn bands(count: u32) -> RenderPlan {
        let bands: Vec<Rect> = (0..count)
            .map(|i| Rect::new(0.0, 40.0 + 80.0 * i as f32, SIDE, 10.0))
            .collect();
        let damage = DamageRegion::collapse_from(
            &bands,
            MergePolicy::DEFAULT,
            Rect::new(0.0, 0.0, SIDE, SIDE),
        );
        RenderPlan {
            clear: RgbaF32::BLACK,
            damage: Damage::Partial(damage),
        }
    }

    fn run(row: Row) -> PaintPath {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        let mut frames = new_frames(WindowToken(1), &shared, config(row.strategy, true));
        let base = spec(PHYSICAL, 1.0);
        let spec = FrameSpec {
            target: TargetKey {
                gives_copy: row.gives_copy,
                ..base.target
            },
            ..base
        };
        frames.note_target(&spec);
        frames.target_outlined = row.outlined;
        let mut app = |ui: &mut Ui| bars(ui, row.backdrops);
        let stamp = frames.stamp(&spec);
        let _ = frames.frame(stamp, &mut app);
        let plan = bands(row.bands);
        let facts = PresentFacts {
            strategy: row.strategy,
            capturing: false,
            backbuffer_fresh: row.fresh,
            promote_share: Some(1.0),
        };
        let ctx = BuildCtx {
            frontend: &mut frontend,
            placing: CutoutPlacing {
                placements: &mut CutoutPlacements::default(),
                pool: &mut AtlasPool::new(super::DEVICE_EDGE),
            },
        };
        let job = frames.build_painted(ctx, facts.path(plan), facts);
        assert_eq!(
            job.fitted.scissors.is_partial(),
            !row.whole,
            "{}: the scissors fit the path",
            row.what
        );
        job.fitted.path
    }

    /// A composed frame past the capture cap builds whole under a whole frame's tickets, so a
    /// backdrop whose capture would still serve captures too: 9 backdrops lost theirs, and the
    /// tenth holds its own until the frame overflows.
    #[test]
    fn a_composed_frame_past_the_cap_captures_every_backdrop() {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        let mut frames = new_frames(WindowToken(1), &shared, config(Retained, true));
        frames.note_target(&spec(PHYSICAL, 1.0));
        let mut app = |ui: &mut Ui| bars(ui, 10);
        let stamp = frames.stamp(&spec(PHYSICAL, 1.0));
        let report = frames.frame(stamp, &mut app);
        frames.compose(&mut frontend, report.plan.expect("a first frame paints"));
        let keys: Vec<BackdropKey> = frames
            .ui
            .frame_scene()
            .backdrops()
            .map(|backdrop| backdrop.key)
            .collect();
        assert_eq!(keys.len(), 10);
        for &key in &keys[..9] {
            frames.backdrop_captures.forget(key);
        }

        frames.compose(&mut frontend, bands(1));

        let held = frontend
            .buffer
            .backdrops
            .iter()
            .filter(|row| row.source == BackdropSource::Held)
            .count();
        assert_eq!(frontend.buffer.backdrops.len(), 10);
        assert_eq!(held, 0);
    }

    /// 8 backdrops fill the capture cap; a ninth paints whole. Under 8 bars, each band splits into
    /// 9 pieces, so 3 bands hold 8 + 27 = 35 scissors of the 40 the cap holds, and 4 hold 44. A
    /// target that loses its kept pixels goes through the backbuffer: partial when that holds the
    /// last frame, else whole. A whole frame after a refusal takes the strategy's own path.
    #[test]
    fn each_refusal_of_a_partial_frame_lands_on_its_path() {
        let copy = Row {
            strategy: BackbufferCopy,
            through_backbuffer: true,
            ..KEPT
        };
        let through = Row {
            through_backbuffer: true,
            ..KEPT
        };
        let desktop = Row {
            strategy: DirectAdaptive,
            fresh: true,
            through_backbuffer: true,
            ..KEPT
        };
        let table = [
            Row {
                what: "desktop: partial through a fresh backbuffer",
                ..desktop
            },
            Row {
                what: "desktop: a stale backbuffer resyncs whole",
                fresh: false,
                whole: true,
                ..desktop
            },
            Row {
                what: "desktop: an outline stays through the backbuffer",
                outlined: true,
                ..desktop
            },
            Row {
                what: "desktop: 8 backdrops fit",
                backdrops: 8,
                ..desktop
            },
            Row {
                what: "desktop: 9 backdrops, whole direct",
                backdrops: 9,
                through_backbuffer: false,
                whole: true,
                ..desktop
            },
            Row {
                what: "desktop: 44 scissors, whole direct",
                backdrops: 8,
                bands: 4,
                through_backbuffer: false,
                whole: true,
                ..desktop
            },
            Row {
                what: "kept: direct and partial",
                ..KEPT
            },
            Row {
                what: "copy: partial through the backbuffer",
                ..copy
            },
            Row {
                what: "a backdrop that copies out stays direct",
                backdrops: 1,
                ..KEPT
            },
            Row {
                what: "8 backdrops fit",
                backdrops: 8,
                ..KEPT
            },
            Row {
                what: "9 backdrops: kept, whole direct",
                backdrops: 9,
                whole: true,
                ..KEPT
            },
            Row {
                what: "9 backdrops: copy, whole",
                backdrops: 9,
                whole: true,
                ..copy
            },
            Row {
                what: "35 scissors fit",
                backdrops: 8,
                bands: 3,
                ..KEPT
            },
            Row {
                what: "44 scissors: kept, whole direct",
                backdrops: 8,
                bands: 4,
                whole: true,
                ..KEPT
            },
            Row {
                what: "44 scissors: copy, whole",
                backdrops: 8,
                bands: 4,
                whole: true,
                ..copy
            },
            Row {
                what: "outlined, fresh",
                outlined: true,
                fresh: true,
                ..through
            },
            Row {
                what: "outlined, stale",
                outlined: true,
                whole: true,
                ..through
            },
            Row {
                what: "no copy, no backdrop",
                gives_copy: false,
                ..KEPT
            },
            Row {
                what: "no copy, a backdrop, fresh",
                gives_copy: false,
                backdrops: 1,
                fresh: true,
                ..through
            },
            Row {
                what: "no copy, a backdrop, stale",
                gives_copy: false,
                backdrops: 1,
                whole: true,
                ..through
            },
        ];
        for row in table {
            let PaintPath { route, plan } = run(row);
            let through_backbuffer = route == Route::ViaBackbuffer;
            assert_eq!(through_backbuffer, row.through_backbuffer, "{}", row.what);
            assert_eq!(
                matches!(plan.damage, Damage::Full),
                row.whole,
                "{}",
                row.what
            );
        }
    }
}

mod capture_path_tests {
    use crate::gpu::frame::submission::Route;
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::material::Material;
    use crate::renderer::frontend::Frontend;
    use crate::shape::Shape;
    use crate::ui::Ui;
    use crate::ui::resources::UiResources;
    use crate::ui::window_frames::present::TargetKey;
    use crate::ui::window_frames::present::present_facts::PaintPath;
    use crate::ui::window_frames::present::tests::new_frames;
    use crate::ui::window_frames::present::tests::{config, spec};
    use crate::ui::window_frames::present::{FrameSpec, PresentStrategy};
    use crate::widget_core::configure::Configure;
    use crate::widgets::panel::Panel;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;

    /// The present path of a fresh window's first frame, whole, under one backdrop at `backdrop` on
    /// a 100 × 80 target that gives a copy when `gives_copy` is.
    fn first_frame(backdrop: Rect, gives_copy: bool) -> PaintPath {
        let shared = UiResources::isolated_mono();
        let mut frontend = Frontend::for_test();
        let mut frames = new_frames(
            WindowToken(1),
            &shared,
            config(PresentStrategy::DirectAdaptive, true),
        );
        let base = spec(UVec2::new(100, 80), 1.0);
        let spec = FrameSpec {
            target: TargetKey {
                gives_copy,
                ..base.target
            },
            ..base
        };
        frames.note_target(&spec);
        let mut app = |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrops")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 4.0, 4.0)),
                        RgbaF32::WHITE,
                    );
                    ui.add_shape(Shape::rect().at(backdrop), Material::blur(2.0));
                });
        };
        let stamp = frames.stamp(&spec);
        frames
            .paint_at(&mut frontend, stamp, &mut app)
            .built
            .path()
            .expect("a first frame paints")
    }

    /// A whole frame paints straight into the target unless its captures cannot copy out of it,
    /// which gives no copy, or would copy as much as the surface holds: then it paints through
    /// the backbuffer, where its blurs read their level 0 in place.
    #[test]
    fn a_capturing_frame_paints_through_the_backbuffer_where_its_captures_outweigh_it() {
        let small = Rect::new(4.0, 4.0, 20.0, 20.0);
        let whole = Rect::new(0.0, 0.0, 100.0, 80.0);
        for (backdrop, gives_copy, through, what) in [
            (small, true, false, "a small capture"),
            (small, false, true, "a target that gives no copy"),
            (whole, true, true, "a capture of the whole surface"),
        ] {
            let path = first_frame(backdrop, gives_copy);
            assert_eq!(
                path.route == Route::ViaBackbuffer,
                through,
                "{what}: {path:?}"
            );
        }
    }
}
