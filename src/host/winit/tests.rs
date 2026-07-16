use crate::Ui;
use crate::display::Display;
use crate::host::winit::{App, AppFrame};
use crate::ui::frame::FrameStamp;
use crate::ui::frame_report::FrameProcessing;
use crate::window::WindowToken;
use glam::UVec2;
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(320, 200);

#[derive(Debug, Default)]
struct CountingApp {
    updates: u32,
    records: u32,
    relayout_on_next_record: bool,
}

impl App for CountingApp {
    fn update(&mut self, win: WindowToken, ui: &Ui) {
        assert_eq!(win, WindowToken(7));
        assert_eq!(ui.display.physical, SURFACE);
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
    let mut app_frame = AppFrame::new(app, WindowToken(7));
    let report = ui.frame(
        FrameStamp::new(Display::from_physical(SURFACE, 1.0), now),
        |ui| app_frame.record(ui),
    );
    ui.frame_runtime.frame_submitted = true;
    report.processing
}

#[test]
fn update_runs_once_when_record_replays() {
    let mut ui = Ui::default();
    let mut app = CountingApp::default();

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
}
