use crate::Ui;
use crate::app::App;
use crate::gpu::error::GpuRequestError;
use crate::gpu::power_preference::PowerPreference;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::{WinitHost, finish_run};
use crate::text::font_scope::FontScope;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::wake::Wake;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::harness::UiHarness;
use crate::window::vsync::Vsync;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(320, 200);

#[derive(Debug, Default)]
struct CountingApp {
    updates: u32,
    records: u32,
    relayout_on_next_record: bool,
    expected_pointer: Option<Vec2>,
}

impl App for CountingApp {
    fn update(&mut self, win: WindowToken, ui: &Ui) {
        assert_eq!(win, WindowToken(0));
        assert_eq!(ui.display().physical, SURFACE);
        assert_eq!(ui.input().pointer_pos, self.expected_pointer);
        self.updates += 1;
    }

    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        assert_eq!(win, WindowToken(0));
        self.records += 1;
        if self.relayout_on_next_record {
            self.relayout_on_next_record = false;
            ui.request_relayout();
        }
    }
}

#[test]
fn builder_retains_defaults_and_granular_overrides() {
    let defaults = WinitHost::<CountingApp>::builder(WindowToken(3));
    assert_eq!(defaults.first_token, WindowToken(3));
    assert_eq!(defaults.config.vsync, Vsync::On);
    assert_eq!(defaults.config.power_preference, PowerPreference::LowPower);
    assert!(!defaults.config.collect_gpu_stats);
    assert!(defaults.config.pixel_snap);

    let builder = WinitHost::<CountingApp>::builder(WindowToken(9))
        .config(WinitHostConfig {
            window: WindowConfig::new("config"),
            vsync: Vsync::Off,
            power_preference: PowerPreference::Any,
            collect_gpu_stats: false,
            fonts: FontScope::Bundled,
            pixel_snap: true,
        })
        .window(WindowConfig::new("window"))
        .title("title")
        .power_preference(PowerPreference::HighPerformance)
        .collect_gpu_stats(true)
        .pixel_snap(false);

    assert_eq!(builder.first_token, WindowToken(9));
    assert_eq!(builder.config.window.title, "title");
    assert_eq!(builder.config.vsync, Vsync::Off);
    assert_eq!(
        builder.config.power_preference,
        PowerPreference::HighPerformance
    );
    assert!(builder.config.collect_gpu_stats);
    assert!(
        !builder.config.pixel_snap,
        "a granular setter overrides what `config` supplied",
    );
}

#[test]
fn run_result_preserves_normal_exit_and_prioritizes_host_failure() {
    assert!(finish_run(None, Ok(())).is_ok());

    let loop_failure =
        finish_run(None, Err(winit::error::EventLoopError::RecreationAttempt)).unwrap_err();
    assert!(matches!(
        loop_failure,
        WinitHostError::RunEventLoop {
            source: winit::error::EventLoopError::RecreationAttempt
        }
    ));

    let host_failure = finish_run(
        Some(GpuRequestError::NoBackend.into()),
        Err(winit::error::EventLoopError::RecreationAttempt),
    )
    .unwrap_err();
    assert!(matches!(
        host_failure,
        WinitHostError::Gpu {
            source: GpuRequestError::NoBackend
        }
    ));
}

#[test]
fn app_lifecycle_follows_frame_plan_and_record_replays() {
    let mut h = UiHarness::cold(SURFACE);
    let mut app = CountingApp::default();
    let pointer = Vec2::new(24.0, 12.0);
    h.move_to(pointer);
    app.expected_pointer = Some(pointer);

    let processing = h.frame_app(&mut app).processing;
    assert_eq!(processing, FrameProcessing::SingleLayout);
    assert_eq!(app.updates, 1, "cold-start frame updates once");
    assert_eq!(app.records, 2, "cold-start warmup and pass A both record");

    app.relayout_on_next_record = true;
    h.ui().request_repaint();
    let processing = h
        .at(Duration::from_millis(16))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::DoubleLayout);
    assert_eq!(app.updates, 2, "relayout frame adds one update");
    assert_eq!(app.records, 4, "relayout frame records pass A and pass B");

    h.ui().frame_runtime_mut().repaint_wakes.push(Wake {
        deadline: Duration::from_millis(32),
        reasons: WakeReasons::ANIM,
    });
    let processing = h
        .at(Duration::from_millis(32))
        .frame_app(&mut app)
        .processing;
    assert_eq!(processing, FrameProcessing::PaintOnly);
    assert_eq!(app.updates, 2, "paint-only frame skips update");
    assert_eq!(app.records, 4, "paint-only frame skips record");
}
