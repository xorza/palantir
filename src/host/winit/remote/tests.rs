//! A remote against a loop of the test's own, which serves a handle's events on a harness of its
//! thread as the event loop serves a window's: a query between two frames with a frame after,
//! a read between two frames with none, sent input before a frame, a shot handed back in the frame
//! it lands in, a frame summary after each frame.

use crate::driver::agent_window::AgentWindow;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use glam::{UVec2, Vec2};

use crate::driver::action::Action;
use crate::driver::error::{LocateError, PlayError};
use crate::driver::locator::{Found, Locator};
use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::HarnessDepth;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::handle::{EventSender, HostHandle, UserEvent};
use crate::host::winit::remote::remote_end::RemoteEnd;
use crate::host::winit::remote::{Remote, Until, WAIT};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::wheel_delta::WheelDelta;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::input::zoom_factor::ZoomFactor;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;
use crate::window::window_token::WindowToken;

const WINDOW: WindowToken = WindowToken(0);
const SIZE: UVec2 = UVec2::new(320, 200);
/// The wait tick the remotes here check at, short so a test that outlasts one stays fast.
const TICK: Duration = Duration::from_millis(5);

/// One button that counts its clicks, a pad that sums the gestures it takes, a chord it
/// counts, a flag a test sets for the work it reports busy, one for an animation that never ends,
/// and one for a second button.
#[derive(Debug, Default)]
struct Counter {
    clicks: u32,
    busy: bool,
    animating: bool,
    gestures: Gestures,
    extra: bool,
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
        if self.animating {
            ui.request_repaint();
        }
        Panel::vstack().auto_id().show(ui, |ui| {
            if Button::new()
                .id(WidgetId::from_hash("press"))
                .label("Press me")
                .show(ui)
                .clicked()
            {
                self.clicks += 1;
            }
            if self.extra {
                Button::new()
                    .id(WidgetId::from_hash("extra"))
                    .label("Extra")
                    .show(ui);
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
            gestures.scroll += pad.scroll.wheel.pixels;
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
    frame_log: Arc<FrameLog>,
    start: Instant,
    repaint: bool,
}

impl Loop {
    fn frame(&mut self) {
        self.h.at(self.start.elapsed());
        let report = self.h.frame_app(&mut self.app);
        let summary = self
            .remote
            .after_frame(self.h.ui(), &report, self.app.is_busy());
        self.repaint = report.repaint_requested;
        if self.repaint {
            self.h.wait_for_gpu();
        }
        self.frame_log.record(WINDOW, summary);
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
                UserEvent::Query(WINDOW, task) => {
                    self.remote.run_query(&mut self.app, self.h.ui_mut(), task);
                }
                UserEvent::Read(WINDOW, task) => {
                    task(&self.app, self.h.ui());
                    continue;
                }
                UserEvent::Input(WINDOW, event) => {
                    self.h.at(self.start.elapsed());
                    self.h.on_input(event.as_event());
                    self.remote.note_sent();
                }
                UserEvent::Shot(WINDOW, ask) => self.remote.ask_shot(self.h.ui_mut(), ask),
                UserEvent::LoseDevice(WINDOW, task) => {
                    let gpu = self.h.gpu().expect("a loop that loses its device paints");
                    self.remote.run_lose_device(gpu, task);
                }
                UserEvent::RunOnMain(task) => {
                    task(&mut self.app);
                }
                UserEvent::Quit => {
                    self.frame_log.remove(WINDOW);
                    return;
                }
                _ => continue,
            }
            self.frame();
        }
    }
}

/// A handle to a loop of the test's own on a thread of its own, with no device: a device is shared
/// by the tests, so a wait for it would hold this window's frames for work of others.
fn serve() -> (HostHandle<Counter>, JoinHandle<()>) {
    serve_at(HarnessDepth::Record)
}

fn serve_at(depth: HarnessDepth) -> (HostHandle<Counter>, JoinHandle<()>) {
    let (sender, events) = mpsc::channel();
    let frame_log = Arc::new(FrameLog::default());
    frame_log.open(WINDOW);
    let handle = HostHandle {
        sender: EventSender::Channel(sender),
        frame_log: Arc::clone(&frame_log),
    };
    let thread = thread::spawn(move || {
        let window = Loop {
            h: Harness::builder(SIZE).depth(depth).build(),
            app: Counter::default(),
            remote: RemoteEnd::default(),
            frame_log,
            start: Instant::now(),
            repaint: false,
        };
        window.serve(&events);
    });
    // The window's first frame, before a remote reads it.
    handle.remote(WINDOW).settle().unwrap();
    (handle, thread)
}

/// A query reads the app and the `Ui` between two frames; play clicks a button by text, once;
/// `find` names the button's rect and center, and refuses a text no widget shows; a shot lands at
/// the window's size; a wait past the remote's own bound gives up; a query to a window that is
/// not open, or after the loop ended, is disconnected.
#[test]
fn a_remote_plays_and_reads_a_live_window() {
    let (handle, thread) = serve_at(HarnessDepth::gpu(headless_test_gpu().handles()));
    let mut remote = handle.remote(WINDOW).with_tick(TICK);

    let (clicks, frame) = remote
        .query(|app, ui| (app.clicks, ui.record_frame_idx()))
        .unwrap();
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
    let button = remote.widgets(Some("press")).unwrap()[0].rect;
    let found = remote
        .find(&Locator::<Infallible>::from("Press me"))
        .unwrap();
    assert_eq!((found.rect, found.point), (button, button.center()));
    assert!(matches!(
        remote.find(&Locator::<Infallible>::from("Missing")),
        Err(PlayError::Locate(LocateError::NotFound { .. }))
    ));

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
    assert_eq!(
        handle.remote(WindowToken(5)).read(|_, _| ()),
        Err(PlayError::Disconnected)
    );

    // A query that outlasts a 50 ms wait gives up after it: the query holds the window's thread
    // until the test lets it go, so no sleep of the test's sets how long it takes.
    let mut short = handle
        .remote(WINDOW)
        .with_tick(TICK)
        .with_wait(Duration::from_millis(50));
    let (release, held_until) = mpsc::channel::<()>();
    assert!(matches!(
        short.query(move |_, _| held_until.recv().ok()),
        Err(PlayError::NoFrame { waited }) if waited >= Duration::from_millis(50)
    ));
    // The thread is still held, so a settle sees no frame at all and says only that.
    assert!(matches!(
        short.settle(),
        Err(PlayError::NoFrame { waited }) if waited >= Duration::from_millis(50)
    ));
    // So does a read, which never runs once the thread lets go.
    let read_late = Arc::new(AtomicBool::new(false));
    let marked = Arc::clone(&read_late);
    assert!(matches!(
        short.read(move |_, _| marked.store(true, Ordering::Release)),
        Err(PlayError::NoFrame { waited }) if waited >= Duration::from_millis(50)
    ));
    release.send(()).ok();

    // A query or a read gives up at once while the window's thread is held, and never runs.
    let mut held = handle.remote(WINDOW).with_tick(TICK).with_blocked(|| true);
    let ran = Arc::new(AtomicBool::new(false));
    let marked = Arc::clone(&ran);
    assert_eq!(
        held.query(move |_, _| marked.store(true, Ordering::Release)),
        Err(PlayError::Blocked)
    );
    let marked = Arc::clone(&ran);
    assert_eq!(
        held.read(move |_, _| marked.store(true, Ordering::Release)),
        Err(PlayError::Blocked)
    );
    remote.query(|_, _| ()).unwrap();
    assert!(
        !ran.load(Ordering::Acquire),
        "a query or a read given up on never runs"
    );
    assert!(
        !read_late.load(Ordering::Acquire),
        "a read that outlasted its wait never runs"
    );
    handle.quit();
    thread.join().unwrap();
    assert_eq!(remote.query(|_, _| ()), Err(PlayError::Disconnected));
    assert_eq!(remote.read(|_, _| ()), Err(PlayError::Disconnected));
}

/// A step whose frame fails while the window's thread is held still lets go: once the hold ends, the
/// window reads no key down and no modifier held. The serving loop runs on under the probe, as a
/// dialog's end lets the queued events through.
#[test]
fn a_step_failed_by_a_held_thread_leaves_nothing_held() {
    let (handle, thread) = serve();
    let held = Arc::new(AtomicBool::new(true));
    let probe = Arc::clone(&held);
    let mut remote = handle
        .remote(WINDOW)
        .with_tick(TICK)
        .with_blocked(move || probe.load(Ordering::Acquire));
    let chord = Shortcut::ctrl('O');
    let held_keys = move |remote: &mut Remote<Counter>| {
        remote
            .query(move |_, ui| (ui.key_held(chord.key), ui.peek_modifiers()))
            .unwrap()
    };
    let steps: [Action; 2] = [Action::Key(chord), Action::KeyDown(chord)];
    for action in steps {
        held.store(true, Ordering::Release);
        let failed = remote.play(std::slice::from_ref(&action)).unwrap_err();
        assert_eq!(failed.step, 0, "{action:?}");
        assert_eq!(failed.source, PlayError::Blocked, "{action:?}");
        held.store(false, Ordering::Release);
        remote.settle().unwrap();
        assert_eq!(
            held_keys(&mut remote),
            (false, Modifiers::NONE),
            "{action:?}"
        );
    }
    handle.quit();
    thread.join().unwrap();
}

/// A window reads as open from its open to its close. A settle ahead of the first frame waits for
/// that frame, and gives up as any wait does, where a window that is not open says so at once.
#[test]
fn a_settle_waits_through_a_windows_first_frame() {
    let (sender, _events) = mpsc::channel();
    let handle: HostHandle<Counter> = HostHandle {
        sender: EventSender::Channel(sender),
        frame_log: Arc::default(),
    };
    let wait = Duration::from_millis(20);
    let mut short = handle.remote(WINDOW).with_tick(TICK).with_wait(wait);
    assert!(!handle.is_open(WINDOW));
    assert_eq!(short.settle(), Err(PlayError::Disconnected));

    handle.frame_log.open(WINDOW);
    assert!(handle.is_open(WINDOW), "before its first frame");
    assert!(matches!(
        short.settle(),
        Err(PlayError::NoFrame { waited }) if waited >= wait
    ));

    // The settle's first check starts the frame, so the frame lands after the settle began.
    let (began, settle_began) = mpsc::channel();
    let mut settling = handle.remote(WINDOW).with_tick(TICK).with_blocked(move || {
        began.send(()).ok();
        false
    });
    let log = Arc::clone(&handle.frame_log);
    let first_frame = thread::spawn(move || {
        settle_began.recv().unwrap();
        log.record(
            WINDOW,
            FrameSummary {
                record_frame_idx: 1,
                repaint_requested: false,
                ..FrameSummary::AT_OPEN
            },
        );
    });
    assert_eq!(settling.settle(), Ok(1));
    first_frame.join().unwrap();

    handle.frame_log.remove(WINDOW);
    assert!(!handle.is_open(WINDOW));
    assert_eq!(settling.settle(), Err(PlayError::Disconnected));
}

/// A remote kept across its window's close and reopen counts against the window as it is open: the
/// events it sent the closed one hold no settle of the new one, whose count starts at zero, and an
/// event it sends the new one counts from there. Thirty seconds bound each wait that must not last.
#[test]
fn a_remote_counts_against_the_window_as_it_is_open_now() {
    const LONG: Duration = Duration::from_secs(30);
    let (sender, _events) = mpsc::channel();
    let handle: HostHandle<Counter> = HostHandle {
        sender: EventSender::Channel(sender),
        frame_log: Arc::default(),
    };
    let idle = |record_frame_idx, sent_events| FrameSummary {
        record_frame_idx,
        sent_events,
        repaint_requested: false,
        busy: false,
    };
    handle.frame_log.open(WINDOW);
    handle.frame_log.record(WINDOW, idle(1, 0));
    let mut remote = handle.remote(WINDOW).with_tick(TICK).with_wait(LONG);
    for _ in 0..3 {
        remote.count_sent(1);
    }
    assert_eq!(remote.sent, 3);
    handle.frame_log.record(WINDOW, idle(2, 3));
    remote.settle().unwrap();

    handle.frame_log.remove(WINDOW);
    handle.frame_log.open(WINDOW);
    handle.frame_log.record(WINDOW, idle(1, 0));
    let began = Instant::now();
    remote.settle().unwrap();
    assert!(
        began.elapsed() < LONG,
        "the three events went to the closed window"
    );

    remote.count_sent(1);
    assert_eq!(
        remote.sent, 1,
        "the new opening's count, 0, and the one sent"
    );
    handle.frame_log.record(WINDOW, idle(2, 1));
    remote.settle().unwrap();
}

/// A query that changes the app shows to the next settle: the frame after it records, and counts
/// the query as a sent event, so the settle waits for that frame and a read finds what the query
/// added.
#[test]
fn a_settle_after_a_query_sees_what_it_changed() {
    let (handle, thread) = serve();
    let mut remote = handle.remote(WINDOW).with_tick(TICK);
    remote.settle().unwrap();
    remote.query(|app, _| app.extra = true).unwrap();
    remote.settle().unwrap();
    assert_eq!(remote.widgets(Some("Extra")).unwrap().len(), 1);
    handle.quit();
    thread.join().unwrap();
}

/// A remote loses the device between two frames, and the loss counts as a sent event: the frame
/// after it saw it, so a settle waits for that frame. The loop here, a harness, opens no device in
/// its place, where a winit host opens one.
#[test]
fn a_remote_loses_the_device_and_a_settle_waits_for_the_frame_after() {
    let gpu = isolated_headless_test_gpu();
    let (handle, thread) = serve_at(HarnessDepth::gpu(gpu.handles()));
    let mut remote = handle.remote(WINDOW).with_tick(TICK);
    remote.settle().unwrap();
    let before = handle.last_frame(WINDOW).unwrap().summary.sent_events;
    assert!(!gpu.is_lost());
    remote.lose_device().unwrap();
    assert!(gpu.is_lost());
    remote.settle().unwrap();
    // 1 = the loss, the one event sent since `before`.
    assert_eq!(
        handle.last_frame(WINDOW).unwrap().summary.sent_events,
        before + 1
    );
    handle.quit();
    thread.join().unwrap();
}

/// A read, a find and a widget list of a hidden window answer, run no frame, count no event and
/// leave the next settle `Ok` with no frame run; a query counts one. A slow query to a hidden window
/// answers. A settle waits out a busy app until a frame finds it idle: work another thread ends
/// after the settle began.
#[test]
fn a_remote_answers_a_hidden_window_and_settles_a_busy_app() {
    let (handle, thread) = serve();
    let mut remote = handle.remote(WINDOW).with_tick(TICK);
    assert_eq!(remote.query(|app, _| app.clicks).unwrap(), 0);
    remote.settle().unwrap();
    let frame = remote.record_frame_idx();
    let sent = remote.sent;
    assert!(frame > 0, "a frame ran before the reads");
    handle.frame_log.set_hidden(WINDOW, true);
    assert_eq!(
        remote.read(|app, ui| (app.clicks, ui.record_frame_idx())),
        Ok((0, frame))
    );
    let button = remote.widgets(Some("press")).unwrap()[0].rect;
    assert_eq!(
        remote.find(&Locator::<Infallible>::from("Press me")),
        Ok(Found::center_of(button))
    );
    assert_eq!(remote.sent, sent);
    assert_eq!(remote.settle(), Ok(0));
    assert_eq!(remote.record_frame_idx(), frame);
    // A query runs between frames, a hidden window's too, so its answer outlasts the ticks that
    // give up a wait for a hidden window's frame.
    let slow = remote.query(|app, _| {
        thread::sleep(TICK * 4);
        app.clicks
    });
    assert_eq!(slow, Ok(0));
    assert_eq!(remote.sent, sent + 1);
    // A frame's answer does not come from a hidden window, and that says so at once. A held thread
    // is the cause first, as no frame runs when the window shows either.
    assert_eq!(remote.shot(None).err(), Some(PlayError::Hidden));
    let mut held = handle.remote(WINDOW).with_tick(TICK).with_blocked(|| true);
    assert_eq!(held.shot(None).err(), Some(PlayError::Blocked));
    handle.frame_log.set_hidden(WINDOW, false);

    remote.settle().unwrap();
    handle.run_on_main(|app| app.busy = true).unwrap();
    remote.query(|app, _| assert!(app.busy)).unwrap();
    // The settle asks whether the window is held at each check it makes while the app is busy,
    // so the first ask starts the work's end after the settle began.
    let (began, settle_began) = mpsc::channel();
    // No wait is too long to bound a busy settle by: the bound saturates, past twenty of these.
    let mut settling = handle
        .remote(WINDOW)
        .with_tick(TICK)
        .with_wait(Duration::MAX)
        .with_blocked(move || {
            began.send(()).ok();
            false
        });
    let worker = handle.clone();
    let work = thread::spawn(move || {
        settle_began.recv().unwrap();
        worker.run_on_main(|app| app.busy = false).unwrap();
    });
    settling.settle().unwrap();
    assert!(
        !remote.query(|app, _| app.busy).unwrap(),
        "the settle waited the work out"
    );
    work.join().unwrap();
    handle.quit();
    thread.join().unwrap();
}

/// A settle that outlasts its wait says which: a busy app that ran no frame reads as busy, a window
/// that holds an event it has not run reads as no frame, whatever its last frame said, and a window
/// that keeps running frames reads as unsettled, with the frames it ran.
#[test]
fn a_settle_that_outlasts_its_wait_says_why() {
    let (handle, thread) = serve();
    let mut remote = handle.remote(WINDOW).with_tick(TICK);
    // Busy and no frame come from nothing happening, so a few ticks prove them.
    let nothing_comes = TICK * 4;

    remote.query(|app, _| app.busy = true).unwrap();
    remote.wait_frame(Until::SawEvents).unwrap();
    remote = remote.with_wait(nothing_comes);
    assert!(matches!(
        remote.settle(),
        Err(PlayError::Busy { waited }) if waited >= nothing_comes
    ));

    // The window's thread holds, known held before the event goes, so the event stays unrun.
    let (holding, held) = mpsc::channel();
    let (release, held_until) = mpsc::channel::<()>();
    handle
        .run_on_main(move |_| {
            holding.send(()).unwrap();
            held_until.recv().ok();
        })
        .unwrap();
    held.recv().unwrap();
    assert!(matches!(
        remote.query(|_, _| ()),
        Err(PlayError::NoFrame { waited }) if waited >= nothing_comes
    ));
    let settled = remote.settle();
    assert!(
        matches!(settled, Err(PlayError::NoFrame { waited }) if waited >= nothing_comes),
        "{settled:?}"
    );
    release.send(()).unwrap();

    // Long enough for a frame on a loaded machine, as an unsettled window must run one.
    remote = remote.with_wait(WAIT);
    remote
        .query(|app, _| {
            app.busy = false;
            app.animating = true;
        })
        .unwrap();
    remote.wait_frame(Until::SawEvents).unwrap();
    remote = remote.with_wait(Duration::from_millis(250));
    let settled = remote.settle();
    assert!(
        matches!(settled, Err(PlayError::Unsettled { frames_recorded }) if frames_recorded > 0),
        "{settled:?}"
    );

    handle.quit();
    thread.join().unwrap();
}

/// What the pad took, read the one way on either host.
fn gestures_of(window: &mut impl AgentWindow<Counter>) -> Gestures {
    window.read(|app, _| app.gestures.clone()).unwrap()
}

/// One table of steps a person takes, played on a [`Harness`] and on a served [`Remote`] alike,
/// each step reading back the same: a drag of 40 px in 8 moves ends once and travels exactly
/// (40, 0); a scroll of (0, 30) px reaches the pad whole; a pinch of 1.5 reaches it; a chord with
/// two modifiers fires
/// the app's read once. A wait of 10 ms runs the harness's stepped clock that far on, and takes
/// that much real time on the remote, whose idle window runs no frame meanwhile, as the desktop
/// host paints only what changes.
#[test]
fn both_players_play_each_gesture_alike() {
    let pad = || Locator::Id(pad_id());
    let steps: [(Action, Result<Gestures, PlayError>); 4] = [
        (
            Action::Drag {
                from: pad(),
                to: pad().offset(Vec2::new(40.0, 0.0)),
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
                delta: WheelDelta::pixels(Vec2::new(0.0, 30.0)),
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
                factor: ZoomFactor::new(1.5).unwrap(),
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
    let mut h = Harness::builder(SIZE)
        .depth(HarnessDepth::gpu(gpu.handles()))
        .build();
    let mut app = Counter::default();
    // The first frame, which the served window runs at its open, before a step locates in it.
    h.frame_app(&mut app);
    let (handle, thread) = serve();
    let mut remote = handle.remote(WINDOW).with_tick(TICK);
    let mut last = Gestures::default();
    for (i, (action, want)) in steps.into_iter().enumerate() {
        let on_harness = h
            .session(&mut app)
            .play(std::slice::from_ref(&action))
            .map(|_| app.gestures.clone())
            .map_err(|failed| failed.source);
        let on_remote = remote
            .play(std::slice::from_ref(&action))
            .map_err(|failed| failed.source)
            .map(|_| gestures_of(&mut remote));
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
            gestures_of(&mut remote),
            gestures_of(&mut h.session(&mut app)),
            "step {i}: the remote's app reads as the harness's"
        );
        last = app.gestures.clone();
    }

    let wait = Duration::from_millis(10);
    let before = h.ui().now();
    h.session(&mut app)
        .play(&[Action::<Infallible>::Wait(wait)])
        .unwrap();
    assert!(
        h.ui().now() >= before + wait,
        "the harness's clock ran {wait:?}"
    );
    let started = Instant::now();
    remote.play(&[Action::<Infallible>::Wait(wait)]).unwrap();
    assert!(
        started.elapsed() >= wait,
        "the remote waited {wait:?} of real time"
    );
    handle.quit();
    thread.join().unwrap();
}
