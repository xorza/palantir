//! The pacing of a window set, on windows and a clock of the test's own: no winit window, no event
//! loop, and every instant an offset from one start.

use crate::host::winit::window::FramePresent;
use crate::host::winit::window_set::{Paced, WindowSet, WindowSlot};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// A window that keeps its next frame and counts the redraws the pacing asks of it.
#[derive(Debug)]
struct FakeWindow {
    next: FramePresent,
    redraws: Cell<u32>,
}

impl Paced for FakeWindow {
    fn next(&self) -> FramePresent {
        self.next
    }

    fn set_next(&mut self, next: FramePresent) {
        self.next = next;
    }

    fn request_redraw(&self) {
        self.redraws.set(self.redraws.get() + 1);
    }
}

fn set(nexts: impl IntoIterator<Item = FramePresent>) -> WindowSet<FakeWindow> {
    WindowSet {
        windows: nexts
            .into_iter()
            .map(|next| FakeWindow {
                next,
                redraws: Cell::new(0),
            })
            .collect(),
    }
}

fn nexts(windows: &WindowSet<FakeWindow>) -> Vec<FramePresent> {
    windows.iter().map(|win| win.next).collect()
}

fn redrawn(windows: &WindowSet<FakeWindow>) -> Vec<usize> {
    windows
        .iter()
        .enumerate()
        .filter(|(_, win)| win.redraws.get() > 0)
        .map(|(idx, _)| idx)
        .collect()
}

fn ms(t0: Instant, ms: u64) -> Instant {
    t0 + Duration::from_millis(ms)
}

/// A wake asks a redraw of each window due now, a follow and a deadline already past included,
/// and hands back the nearest deadline still ahead.
#[test]
fn a_wake_redraws_what_is_due_and_returns_the_nearest_deadline() {
    let t0 = Instant::now();
    let at = |offset| FramePresent::At(ms(t0, offset));
    for (label, presents, want_redrawn, want_wake) in [
        ("none", vec![], vec![], None),
        ("idle", vec![FramePresent::Idle], vec![], None),
        (
            "nearest of three",
            vec![at(30), at(10), at(20)],
            vec![],
            Some(ms(t0, 10)),
        ),
        (
            "mixed",
            vec![
                FramePresent::Immediate,
                at(40),
                FramePresent::Idle,
                FramePresent::Immediate,
            ],
            vec![0, 3],
            Some(ms(t0, 40)),
        ),
        (
            "a follow redraws as an immediate does",
            vec![FramePresent::Idle, FramePresent::Follow, at(5)],
            vec![1],
            Some(ms(t0, 5)),
        ),
        (
            "a deadline at now is due",
            vec![at(0), at(5)],
            vec![0],
            Some(ms(t0, 5)),
        ),
    ] {
        let windows = set(presents);
        assert_eq!(windows.wake(t0), want_wake, "{label}: wake");
        assert_eq!(redrawn(&windows), want_redrawn, "{label}: redrawn");
    }
}

/// A frame as a window runs one: it sets its own next frame and answers whether it ran the app.
fn frame(next: FramePresent, ran_app: bool) -> impl FnOnce(&mut FakeWindow) -> Option<bool> {
    move |win| {
        win.next = next;
        Some(ran_app)
    }
}

/// A window's own frame that ran the app makes every other window follow, as of the clock after
/// it: an idle one and one waiting on a deadline are due now, as a follow, and one due for itself
/// stays due for itself. The window that ran keeps the next frame its frame set.
#[test]
fn a_frame_of_its_own_that_ran_the_app_makes_the_others_follow() {
    let t0 = Instant::now();
    let mut windows = set([
        FramePresent::Immediate,
        FramePresent::Idle,
        FramePresent::At(ms(t0, 50)),
        FramePresent::Immediate,
    ]);
    let after = Cell::new(None);
    windows.run(WindowSlot(0), t0, frame(FramePresent::Idle, true), || {
        after.set(Some(ms(t0, 1)));
        ms(t0, 1)
    });
    assert_eq!(
        after.get(),
        Some(ms(t0, 1)),
        "the clock is read after the frame"
    );
    assert_eq!(
        nexts(&windows),
        [
            FramePresent::Idle,
            FramePresent::Follow,
            FramePresent::Follow,
            FramePresent::Immediate,
        ]
    );
}

/// No window follows a frame that changed nothing another window shows: a paint-only frame, as a
/// caret's blink, ran none of the app; a skipped frame ran nothing; and a follow frame only showed
/// another's change, read as it was due before the frame, though the frame then asks for another
/// of its own. So two windows cannot keep each other running.
#[test]
fn a_paint_only_skipped_or_follow_frame_asks_no_window_to_follow() {
    let t0 = Instant::now();
    let unread = || -> Instant { panic!("no window follows, so the clock is not read") };
    let mut windows = set([FramePresent::At(t0), FramePresent::Idle]);
    windows.run(WindowSlot(0), t0, frame(FramePresent::Idle, false), unread);
    assert_eq!(nexts(&windows), [FramePresent::Idle; 2], "a blink");

    let mut windows = set([FramePresent::Immediate, FramePresent::Idle]);
    windows.run(WindowSlot(0), t0, |_| None, unread);
    assert_eq!(
        nexts(&windows),
        [FramePresent::Immediate, FramePresent::Idle],
        "a skipped frame"
    );

    let mut windows = set([FramePresent::Follow, FramePresent::Idle]);
    windows.run(
        WindowSlot(0),
        t0,
        frame(FramePresent::Immediate, true),
        unread,
    );
    assert_eq!(
        nexts(&windows),
        [FramePresent::Immediate, FramePresent::Idle],
        "a follow frame that asks for another"
    );
    assert_eq!(windows.wake(t0), None);
    assert_eq!(redrawn(&windows), [0], "only its own next frame");
}

/// A device recovery holds every window until its backoff ends, whatever each was due for: a wake
/// before then redraws none and waits for the end, and the wake at it redraws all. A recovery that
/// holds then follows: each window due now.
#[test]
fn a_recovery_holds_every_window_then_each_follows() {
    let t0 = Instant::now();
    let mut windows = set([
        FramePresent::Immediate,
        FramePresent::Idle,
        FramePresent::At(ms(t0, 500)),
    ]);
    windows.hold_until(ms(t0, 100));
    assert_eq!(windows.wake(ms(t0, 99)), Some(ms(t0, 100)));
    assert_eq!(redrawn(&windows), Vec::<usize>::new(), "held");
    assert_eq!(windows.wake(ms(t0, 100)), None);
    assert_eq!(redrawn(&windows), [0, 1, 2], "the backoff ended");

    let mut windows = set([
        FramePresent::Immediate,
        FramePresent::Idle,
        FramePresent::At(ms(t0, 500)),
    ]);
    windows.follow_all(t0);
    assert_eq!(
        nexts(&windows),
        [
            FramePresent::Immediate,
            FramePresent::Follow,
            FramePresent::Follow,
        ]
    );
}
