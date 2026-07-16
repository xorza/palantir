use crate::Ui;
use crate::app::App;
use crate::display::Display;
use crate::input::InputEvent;
use crate::ui::frame::{FrameStamp, Wake, WakeReasons};
use crate::ui::frame_report::FrameProcessing;
use crate::window::WindowToken;
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
        assert_eq!(ui.display.physical, SURFACE);
        assert_eq!(ui.input.pointer_pos, self.expected_pointer);
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

fn run_frame(ui: &mut Ui, app: &mut CountingApp, now: Duration) -> FrameProcessing {
    let report = ui.frame(
        FrameStamp::new(Display::from_physical(SURFACE, 1.0), now),
        WindowToken(7),
        app,
    );
    ui.frame_runtime.frame_submitted = true;
    report.processing
}

#[test]
fn app_lifecycle_follows_frame_plan_and_record_replays() {
    let mut ui = Ui::default();
    let mut app = CountingApp::default();
    let pointer = Vec2::new(24.0, 12.0);
    ui.on_input(InputEvent::PointerMoved(pointer));
    app.expected_pointer = Some(pointer);

    let processing = run_frame(&mut ui, &mut app, Duration::ZERO);
    assert_eq!(processing, FrameProcessing::SingleLayout);
    assert_eq!(app.updates, 1, "cold-start frame updates once");
    assert_eq!(app.records, 2, "cold-start warmup and pass A both record");

    app.relayout_on_next_record = true;
    ui.request_repaint();
    let processing = run_frame(&mut ui, &mut app, Duration::from_millis(16));
    assert_eq!(processing, FrameProcessing::DoubleLayout);
    assert_eq!(app.updates, 2, "relayout frame adds one update");
    assert_eq!(app.records, 4, "relayout frame records pass A and pass B");

    ui.frame_runtime.repaint_wakes.push(Wake {
        deadline: Duration::from_millis(32),
        reasons: WakeReasons::ANIM,
    });
    let processing = run_frame(&mut ui, &mut app, Duration::from_millis(32));
    assert_eq!(processing, FrameProcessing::PaintOnly);
    assert_eq!(app.updates, 2, "paint-only frame skips update");
    assert_eq!(app.records, 4, "paint-only frame skips record");
}
