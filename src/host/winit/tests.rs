use crate::Ui;
use crate::app::App;
use crate::display::Display;
use crate::host::error::GpuRequestError;
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::host::winit::{WinitHost, finish_run};
use crate::input::input_event::InputEvent;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::wake::Wake;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::frame_stamp::FrameInput;
use crate::ui::frame_stamp::FrameStamp;
use crate::ui::resources::UiResources;
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
        assert_eq!(win, WindowToken(7));
        assert_eq!(ui.display().physical, SURFACE);
        assert_eq!(ui.input().pointer_pos, self.expected_pointer);
        self.updates += 1;
    }

    fn record(&mut self, win: WindowToken, ui: &mut Ui) {
        assert_eq!(win, WindowToken(7));
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
    assert_eq!(defaults.config.present_mode, wgpu::PresentMode::AutoVsync);
    assert_eq!(
        defaults.config.power_preference,
        wgpu::PowerPreference::LowPower
    );
    assert!(!defaults.config.collect_gpu_stats);
    assert!(defaults.config.pixel_snap);

    let builder = WinitHost::<CountingApp>::builder(WindowToken(9))
        .config(WinitHostConfig {
            window: WindowConfig::new("config"),
            present_mode: wgpu::PresentMode::Fifo,
            power_preference: wgpu::PowerPreference::None,
            collect_gpu_stats: false,
            pixel_snap: true,
        })
        .window(WindowConfig::new("window"))
        .title("title")
        .present_mode(wgpu::PresentMode::Immediate)
        .power_preference(wgpu::PowerPreference::HighPerformance)
        .collect_gpu_stats(true)
        .pixel_snap(false);

    assert_eq!(builder.first_token, WindowToken(9));
    assert_eq!(builder.config.window.title, "title");
    assert_eq!(builder.config.present_mode, wgpu::PresentMode::Immediate);
    assert_eq!(
        builder.config.power_preference,
        wgpu::PowerPreference::HighPerformance
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

fn run_frame(
    ui: &mut Ui,
    engines: &mut FrameEngines,
    app: &mut CountingApp,
    now: Duration,
) -> FrameProcessing {
    let report = ui.frame(
        engines,
        FrameInput {
            stamp: FrameStamp::new(Display::from_physical(SURFACE, 1.0), now),
            damage_baseline_valid: true,
        },
        WindowToken(7),
        app,
    );
    report.processing
}

#[test]
fn app_lifecycle_follows_frame_plan_and_record_replays() {
    let resources = UiResources::isolated_mono();
    let mut engines = FrameEngines::new(&resources);
    let mut ui = Ui::new(resources);
    let mut app = CountingApp::default();
    let pointer = Vec2::new(24.0, 12.0);
    ui.on_input(InputEvent::PointerMoved(pointer));
    app.expected_pointer = Some(pointer);

    let processing = run_frame(&mut ui, &mut engines, &mut app, Duration::ZERO);
    assert_eq!(processing, FrameProcessing::SingleLayout);
    assert_eq!(app.updates, 1, "cold-start frame updates once");
    assert_eq!(app.records, 2, "cold-start warmup and pass A both record");

    app.relayout_on_next_record = true;
    ui.request_repaint();
    let processing = run_frame(&mut ui, &mut engines, &mut app, Duration::from_millis(16));
    assert_eq!(processing, FrameProcessing::DoubleLayout);
    assert_eq!(app.updates, 2, "relayout frame adds one update");
    assert_eq!(app.records, 4, "relayout frame records pass A and pass B");

    ui.frame_runtime_mut().repaint_wakes.push(Wake {
        deadline: Duration::from_millis(32),
        reasons: WakeReasons::ANIM,
    });
    let processing = run_frame(&mut ui, &mut engines, &mut app, Duration::from_millis(32));
    assert_eq!(processing, FrameProcessing::PaintOnly);
    assert_eq!(app.updates, 2, "paint-only frame skips update");
    assert_eq!(app.records, 4, "paint-only frame skips record");
}
