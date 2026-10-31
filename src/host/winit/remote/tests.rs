//! A remote against a loop of the test's own, which serves a handle's events on a GPU harness of
//! its thread as the event loop serves a window's: a query between two frames, sent input before
//! a frame, a shot handed back in the frame it lands in, a frame summary after each frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use glam::UVec2;

use crate::app::App;
use crate::driver::action::Action;
use crate::driver::error::PlayError;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::handle::{EventSender, HostHandle, UserEvent};
use crate::host::winit::remote::RemoteShots;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::window::window_token::WindowToken;

const WINDOW: WindowToken = WindowToken(0);
const SIZE: UVec2 = UVec2::new(320, 200);

/// One button that counts its clicks, and a flag a test sets for the work it reports busy.
#[derive(Debug, Default)]
struct Counter {
    clicks: u32,
    busy: bool,
}

impl App for Counter {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        if Button::new()
            .id(WidgetId::from_hash("press"))
            .label("Press me")
            .show(ui)
            .clicked()
        {
            self.clicks += 1;
        }
    }

    fn is_busy(&self) -> bool {
        self.busy
    }
}

/// The window's frames on real time, as the event loop runs them: the harness's clock is set
/// to the time since the loop began before each frame and each event.
struct Loop {
    h: Harness,
    app: Counter,
    shots: RemoteShots,
    frames: Arc<FrameLog>,
    sent: u64,
    start: Instant,
    repaint: bool,
}

impl Loop {
    fn frame(&mut self) {
        self.h.at(self.start.elapsed());
        let report = self.h.frame_app(&mut self.app);
        self.shots.deliver(self.h.ui_ref());
        self.repaint = report.repaint_requested;
        if self.repaint {
            self.h.wait_for_gpu();
        }
        self.frames.record(
            WINDOW,
            FrameSummary {
                frame_id: self.h.ui_ref().frame_id(),
                sent_inputs: self.sent,
                repaint_requested: report.repaint_requested,
                repaint_after: report.repaint_after,
                busy: self.app.is_busy(),
            },
        );
    }

    /// Serve `events` until a quit: an event of another window is dropped, as no window holds it.
    fn serve(mut self, events: &Receiver<UserEvent<Counter>>) {
        self.frame();
        loop {
            let event = if self.repaint {
                match events.try_recv() {
                    Ok(event) => event,
                    Err(TryRecvError::Empty) => {
                        self.frame();
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match events.recv() {
                    Ok(event) => event,
                    Err(_) => return,
                }
            };
            match event {
                UserEvent::Query(WINDOW, task) => task(&mut self.app, self.h.ui()),
                UserEvent::Input(WINDOW, event) => {
                    self.h.at(self.start.elapsed());
                    self.h.on_input(event.as_event());
                    self.sent += 1;
                }
                UserEvent::Shot(WINDOW, ask) => self.shots.ask(self.h.ui(), ask),
                UserEvent::RunOnMain(task) => {
                    task(&mut self.app);
                }
                UserEvent::Quit => {
                    self.frames.remove(WINDOW);
                    return;
                }
                _ => continue,
            }
            self.frame();
        }
    }
}

/// A handle to a loop of the test's own on a thread of its own.
fn serve() -> (HostHandle<Counter>, JoinHandle<()>) {
    let (sender, events) = mpsc::channel();
    let frames = Arc::new(FrameLog::default());
    let handle = HostHandle {
        sender: EventSender::Channel(sender),
        frames: Arc::clone(&frames),
    };
    let thread = thread::spawn(move || {
        let gpu = headless_test_gpu();
        let window = Loop {
            h: Harness::builder(SIZE).gpu(gpu.handles()).build(),
            app: Counter::default(),
            shots: RemoteShots::default(),
            frames,
            sent: 0,
            start: Instant::now(),
            repaint: false,
        };
        window.serve(&events);
    });
    // The window's first frame, before a remote reads it.
    let start = Instant::now();
    while handle.last_frame(WINDOW).is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the loop ran no frame"
        );
        thread::sleep(Duration::from_millis(1));
    }
    (handle, thread)
}

/// A query reads the app and the `Ui` between two frames; play clicks a button by text, once;
/// a shot lands at the window's size; a query to a window that is not open, or after the loop
/// ended, is disconnected.
#[test]
fn a_remote_plays_and_reads_a_live_window() {
    let (handle, thread) = serve();
    let remote = handle.remote(WINDOW);

    let (clicks, frame) = remote.query(|app, ui| (app.clicks, ui.frame_id())).unwrap();
    assert_eq!(clicks, 0);
    assert!(frame > 0, "a frame ran before the query");

    let frames = remote.play(&[Action::click("Press me")]).unwrap();
    assert!(frames > 0);
    assert_eq!(remote.query(|app, _| app.clicks).unwrap(), 1);
    let texts: Vec<String> = remote
        .widgets(Some("press".to_owned()))
        .unwrap()
        .into_iter()
        .map(|widget| widget.text)
        .collect();
    assert_eq!(texts, ["Press me"]);

    let shot = remote.shot(None).unwrap();
    assert_eq!(shot.image().size(), SIZE);
    assert_eq!(
        remote.shot(Some(Rect::new(900.0, 900.0, 4.0, 4.0))),
        Err(PlayError::EmptyRegion)
    );

    assert_eq!(
        handle.remote(WindowToken(5)).query(|_, _| ()),
        Err(PlayError::Disconnected)
    );

    // A wait gives up at once while the window's thread is held, and the query it gave up on
    // never runs, though the loop serves it later.
    handle
        .run_on_main(|_| {
            thread::sleep(Duration::from_millis(100));
            false
        })
        .unwrap();
    let held = handle.remote(WINDOW).with_blocked(|| true);
    let ran = Arc::new(AtomicBool::new(false));
    let marked = Arc::clone(&ran);
    assert!(matches!(
        held.query(move |_, _| marked.store(true, Ordering::Release)),
        Err(PlayError::NoFrame { .. })
    ));
    remote.query(|_, _| ()).unwrap();
    assert!(
        !ran.load(Ordering::Acquire),
        "a query given up on never runs"
    );
    handle.quit();
    thread.join().unwrap();
    assert_eq!(remote.query(|_, _| ()), Err(PlayError::Disconnected));
}

/// A query that panics answers with the panic's message, and the window goes on to answer the
/// next. A settle waits out a busy app until a frame finds it idle: work another thread ends
/// after the settle began.
#[test]
fn a_remote_survives_a_panicking_query_and_settles_a_busy_app() {
    let (handle, thread) = serve();
    let remote = handle.remote(WINDOW);
    assert_eq!(
        remote.query(|_, _| -> u32 { panic!("boom") }),
        Err(PlayError::Panicked {
            message: "boom".to_owned()
        })
    );
    assert_eq!(remote.query(|app, _| app.clicks).unwrap(), 0);

    remote.settle().unwrap();
    handle
        .run_on_main(|app| {
            app.busy = true;
            true
        })
        .unwrap();
    remote.query(|app, _| assert!(app.busy)).unwrap();
    let worker = handle.clone();
    let work = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        worker
            .run_on_main(|app| {
                app.busy = false;
                true
            })
            .unwrap();
    });
    remote.settle().unwrap();
    assert!(
        !remote.query(|app, _| app.busy).unwrap(),
        "the settle waited the work out"
    );
    work.join().unwrap();
    handle.quit();
    thread.join().unwrap();
}
