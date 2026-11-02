//! A remote against a loop of the test's own, which serves a handle's events on a GPU harness of
//! its thread as the event loop serves a window's: a query between two frames, sent input before
//! a frame, a shot handed back in the frame it lands in, a frame summary after each frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use glam::{UVec2, Vec2};

use crate::app::App;
use crate::driver::action::Action;
use crate::driver::error::PlayError;
use crate::driver::locator::Locator;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::host::harness::Harness;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::handle::{EventSender, HostHandle, UserEvent};
use crate::host::winit::remote::remote_end::RemoteEnd;
use crate::input::pointer::PointerButton;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::window::window_token::WindowToken;

const WINDOW: WindowToken = WindowToken(0);
const SIZE: UVec2 = UVec2::new(320, 200);

/// One button that counts its clicks, a pad that sums the gestures it takes, a chord it
/// counts, and a flag a test sets for the work it reports busy.
#[derive(Debug, Default)]
struct Counter {
    clicks: u32,
    busy: bool,
    gestures: Gestures,
}

/// What the pad took: drags ended and the travel of the last, the scroll, each pinch, and the
/// presses of [`CHORD`].
#[derive(Clone, Debug, Default, PartialEq)]
struct Gestures {
    drags: u32,
    travel: Vec2,
    scroll: Vec2,
    pinches: Vec<f32>,
    chords: u32,
}

const CHORD: &str = "ctrl+shift+k";

fn pad_id() -> WidgetId {
    WidgetId::from_hash("pad")
}

impl App for Counter {
    fn record(&mut self, _window: WindowToken, ui: &mut Ui) {
        Panel::vstack().auto_id().show(ui, |ui| {
            if Button::new()
                .id(WidgetId::from_hash("press"))
                .label("Press me")
                .show(ui)
                .clicked()
            {
                self.clicks += 1;
            }
            let pad = Panel::hstack()
                .id(pad_id())
                .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
                .sense(Sense::DRAG | Sense::SCROLL | Sense::PINCH)
                .show(ui, |_| {})
                .response;
            let gestures = &mut self.gestures;
            if let Some(delta) = pad.left.drag.delta() {
                gestures.travel = delta;
            }
            gestures.drags += u32::from(pad.left.drag.stopped());
            gestures.scroll += pad.scroll.pixels;
            if pad.scroll.zoom.get() != 1.0 {
                gestures.pinches.push(pad.scroll.zoom.get());
            }
        });
        if ui.shortcut_pressed(CHORD.parse::<Shortcut>().unwrap()) {
            self.gestures.chords += 1;
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
    remote: RemoteEnd,
    frames: Arc<FrameLog>,
    start: Instant,
    repaint: bool,
}

impl Loop {
    fn frame(&mut self) {
        self.h.at(self.start.elapsed());
        let report = self.h.frame_app(&mut self.app);
        let summary = self
            .remote
            .after_frame(self.h.ui_ref(), &report, self.app.is_busy());
        self.repaint = report.repaint_requested;
        if self.repaint {
            self.h.wait_for_gpu();
        }
        self.frames.record(WINDOW, summary);
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
                    self.remote.note_input();
                }
                UserEvent::Shot(WINDOW, ask) => self.remote.ask_shot(self.h.ui(), ask),
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
            remote: RemoteEnd::default(),
            frames,
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
        .widgets(Some("press"))
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

    // A query gives up at once while the window's thread is held, and never runs.
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
/// next; a slow query to a hidden window answers. A settle waits out a busy app until a frame
/// finds it idle: work another thread ends after the settle began.
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
    // A query runs between frames, a hidden window's too, so its answer outlasts the 50 ms tick
    // that gives up a wait for a hidden window's frame.
    handle.frames.set_hidden(WINDOW, true);
    let slow = remote.query(|app, _| {
        thread::sleep(Duration::from_millis(120));
        app.clicks
    });
    assert_eq!(slow, Ok(0));
    handle.frames.set_hidden(WINDOW, false);

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

/// One table of steps a person takes, played on a [`Harness`] and on a served [`Remote`] alike,
/// each step reading back the same: a drag of 40 px in 8 moves ends once and travels exactly
/// (40, 0); a scroll of (0, 30) px reaches the pad whole; a pinch of 1.5 reaches it, one that is
/// not a positive factor fails as `BadPinch` and leaves nothing; a chord with two modifiers fires
/// the app's read once. A wait of 100 ms runs the harness's stepped clock that far on, and takes
/// that much real time on the remote, whose idle window runs no frame meanwhile, as the desktop
/// host paints only what changes.
#[test]
fn both_players_play_each_gesture_alike() {
    let pad = || Locator::Id(pad_id());
    let steps: [(Action, Result<Gestures, PlayError>); 5] = [
        (
            Action::Drag {
                from: pad(),
                to: Locator::Offset(Box::new(pad()), Vec2::new(40.0, 0.0)),
                button: PointerButton::Left,
                mods: ShortcutMods::NONE,
                steps: Action::DRAG_STEPS,
            },
            Ok(Gestures {
                drags: 1,
                travel: Vec2::new(40.0, 0.0),
                ..Gestures::default()
            }),
        ),
        (
            Action::Scroll {
                at: pad(),
                delta: Vec2::new(0.0, 30.0),
            },
            Ok(Gestures {
                drags: 1,
                travel: Vec2::new(40.0, 0.0),
                scroll: Vec2::new(0.0, 30.0),
                ..Gestures::default()
            }),
        ),
        (
            Action::Pinch {
                at: pad(),
                factor: 1.5,
            },
            Ok(Gestures {
                drags: 1,
                travel: Vec2::new(40.0, 0.0),
                scroll: Vec2::new(0.0, 30.0),
                pinches: vec![1.5],
                ..Gestures::default()
            }),
        ),
        (
            Action::Pinch {
                at: pad(),
                factor: -1.0,
            },
            Err(PlayError::BadPinch { factor: -1.0 }),
        ),
        (
            Action::Key(CHORD.parse().unwrap()),
            Ok(Gestures {
                drags: 1,
                travel: Vec2::new(40.0, 0.0),
                scroll: Vec2::new(0.0, 30.0),
                pinches: vec![1.5],
                chords: 1,
            }),
        ),
    ];
    let gpu = headless_test_gpu();
    let mut h = Harness::builder(SIZE).gpu(gpu.handles()).build();
    let mut app = Counter::default();
    // The first frame, which the served window runs at its open, before a step locates in it.
    h.frame_app(&mut app);
    let (handle, thread) = serve();
    let remote = handle.remote(WINDOW);
    let mut last = Gestures::default();
    for (i, (action, want)) in steps.into_iter().enumerate() {
        let on_harness = h
            .play(&mut app, std::slice::from_ref(&action))
            .map(|_| app.gestures.clone())
            .map_err(|failed| failed.error);
        let on_remote = remote
            .play(std::slice::from_ref(&action))
            .map_err(|failed| failed.error)
            .and_then(|_| remote.query(|app, _| app.gestures.clone()));
        let outcome = |result: &Result<Gestures, PlayError>| {
            result.as_ref().map(|_| ()).map_err(Clone::clone)
        };
        assert_eq!(
            outcome(&on_remote),
            outcome(&on_harness),
            "step {i}: the players agree"
        );
        if want.is_ok() {
            assert_eq!(on_harness, want, "step {i} on the harness");
        } else {
            assert_eq!(outcome(&on_harness), outcome(&want), "step {i}");
            assert_eq!(
                app.gestures, last,
                "step {i}: a refused step leaves nothing"
            );
        }
        assert_eq!(
            remote.query(|app, _| app.gestures.clone()).unwrap(),
            app.gestures,
            "step {i}: the remote's app reads as the harness's"
        );
        last = app.gestures.clone();
    }

    let wait = Duration::from_millis(100);
    let before = h.ui_ref().now();
    h.play(&mut app, &[Action::<std::convert::Infallible>::Wait(wait)])
        .unwrap();
    assert!(
        h.ui_ref().now() >= before + wait,
        "the harness's clock ran {wait:?}"
    );
    let started = Instant::now();
    remote
        .play(&[Action::<std::convert::Infallible>::Wait(wait)])
        .unwrap();
    assert!(
        started.elapsed() >= wait,
        "the remote waited {wait:?} of real time"
    );
    handle.quit();
    thread.join().unwrap();
}
