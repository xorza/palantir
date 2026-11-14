//! [`FrameLog`]: each open window's last frame, shared between the event loop and its handles.

use crate::common::sync::{CondvarExt, MutexExt};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::host::winit::frame_summary::FrameSummary;
use crate::window::window_token::WindowToken;

/// The event loop adds a window as it opens, writes its summary after each of its frames and
/// whether it is hidden as that changes, and removes it as it closes, so an entry means an open
/// window; a [`HostHandle`](crate::HostHandle) reads them on any thread, and a waiter wakes on each
/// change. A host has a few windows, so a scan of a short list under one lock is all a read or a
/// write pays.
#[derive(Debug, Default)]
pub(super) struct FrameLog {
    windows: Mutex<Windows>,
    /// Signaled on each write, so a [`Remote`](crate::Remote) waits for a frame without a poll.
    changed: Condvar,
}

/// The open windows, and how many openings the log has numbered.
#[derive(Debug, Default)]
struct Windows {
    logged: Vec<LoggedWindow>,
    openings: u64,
}

#[derive(Clone, Copy, Debug)]
struct LoggedWindow {
    token: WindowToken,
    frame: LoggedFrame,
    hidden: bool,
}

/// A window's last frame, and which opening of a window under its token ran it: a window opened
/// again counts its events and frames from zero, so a count read against an earlier opening says
/// nothing of this one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoggedFrame {
    /// This opening's number, unique in the log: 1 for the first window the log opened, and one
    /// more for each opening after.
    pub(crate) opening: u64,
    pub(crate) summary: FrameSummary,
}

impl FrameLog {
    /// Note that `token`'s window opened.
    ///
    /// # Panics
    ///
    /// When it is open already: a host opens a token once at a time.
    pub(super) fn open(&self, token: WindowToken) {
        let mut windows = self.windows.lock_unpoisoned();
        assert!(
            windows.logged.iter().all(|logged| logged.token != token),
            "{token:?} opened twice"
        );
        windows.openings += 1;
        let opening = windows.openings;
        windows.logged.push(LoggedWindow {
            token,
            frame: LoggedFrame {
                opening,
                summary: FrameSummary::AT_OPEN,
            },
            hidden: false,
        });
        drop(windows);
        self.changed.notify_all();
    }

    pub(super) fn record(&self, token: WindowToken, summary: FrameSummary) {
        Self::entry(&mut self.windows.lock_unpoisoned(), token)
            .frame
            .summary = summary;
        self.changed.notify_all();
    }

    /// Block while `token`'s window is shown and its last frame is `seen`, for `timeout` at most.
    /// The check and the wait share the lock, so a write after the caller read `seen` wakes it.
    pub(super) fn wait_changed(&self, token: WindowToken, seen: LoggedFrame, timeout: Duration) {
        let windows = self.windows.lock_unpoisoned();
        drop(
            self.changed
                .wait_timeout_while_unpoisoned(windows, timeout, |windows| {
                    windows.logged.iter().any(|logged| {
                        logged.token == token && logged.frame == seen && !logged.hidden
                    })
                }),
        );
    }

    /// Note whether `token`'s window is hidden, which holds its frames until it shows.
    pub(super) fn set_hidden(&self, token: WindowToken, hidden: bool) {
        Self::entry(&mut self.windows.lock_unpoisoned(), token).hidden = hidden;
        self.changed.notify_all();
    }

    /// `token`'s window's last frame, [`FrameSummary::AT_OPEN`] until it runs one, or `None` when
    /// it is not open.
    pub(super) fn get(&self, token: WindowToken) -> Option<LoggedFrame> {
        let windows = self.windows.lock_unpoisoned();
        windows
            .logged
            .iter()
            .find(|logged| logged.token == token)
            .map(|logged| logged.frame)
    }

    pub(super) fn is_hidden(&self, token: WindowToken) -> bool {
        let windows = self.windows.lock_unpoisoned();
        windows
            .logged
            .iter()
            .any(|logged| logged.token == token && logged.hidden)
    }

    fn entry(windows: &mut Windows, token: WindowToken) -> &mut LoggedWindow {
        windows
            .logged
            .iter_mut()
            .find(|logged| logged.token == token)
            .expect("the loop writes a window's entry only while it is open")
    }

    pub(super) fn remove(&self, token: WindowToken) {
        self.windows
            .lock_unpoisoned()
            .logged
            .retain(|logged| logged.token != token);
        self.changed.notify_all();
    }

    /// Forget every window, as the event loop ends and closes them all.
    pub(super) fn clear(&self) {
        self.windows.lock_unpoisoned().logged.clear();
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::frame_log::{FrameLog, LoggedFrame};
    use crate::host::winit::frame_summary::FrameSummary;
    use crate::window::window_token::WindowToken;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    const fn summary(record_frame_id: u64, repaint_requested: bool) -> FrameSummary {
        FrameSummary {
            record_frame_id,
            sent_events: 0,
            repaint_requested,
            busy: false,
        }
    }

    /// What the log holds for a window at `opening` whose last frame is `summary`.
    const fn logged(opening: u64, summary: FrameSummary) -> LoggedFrame {
        LoggedFrame { opening, summary }
    }

    /// An entry lasts from a window's open to its close, and each window keeps its own: a record or
    /// a hide changes only that window's, a remove forgets only that window, a closed token opens
    /// again from the start under a new opening, and a clear forgets all. Openings number from 1:
    /// `a` is 1, `b` 2, and `a` opened again 3.
    #[test]
    fn each_window_keeps_its_entry_from_open_to_close() {
        let log = FrameLog::default();
        let (a, b) = (WindowToken(1), WindowToken(2));
        assert_eq!(log.get(a), None, "not open");

        log.open(a);
        log.open(b);
        assert_eq!(
            log.get(a),
            Some(logged(1, FrameSummary::AT_OPEN)),
            "no frame yet"
        );
        log.record(a, summary(1, true));
        log.record(b, summary(1, false));
        let later = summary(2, false);
        log.record(a, later);
        assert_eq!(log.get(a), Some(logged(1, later)));
        assert_eq!(log.get(b), Some(logged(2, summary(1, false))));

        // Hiding leaves the last frame as it was, and is each window's own.
        log.set_hidden(a, true);
        assert_eq!(
            (log.get(a), log.is_hidden(a), log.is_hidden(b)),
            (Some(logged(1, later)), true, false)
        );
        log.set_hidden(a, false);
        assert!(!log.is_hidden(a));

        log.set_hidden(a, true);
        log.remove(a);
        assert_eq!(
            (log.get(a), log.is_hidden(a)),
            (None, false),
            "a closed window is not hidden"
        );
        assert_eq!(log.get(b), Some(logged(2, summary(1, false))));

        log.open(a);
        assert_eq!(
            (log.get(a), log.is_hidden(a)),
            (Some(logged(3, FrameSummary::AT_OPEN)), false),
            "it opens again from the start, under a new opening"
        );

        log.clear();
        assert_eq!([log.get(a), log.get(b)], [None, None]);
    }

    /// A waiter holds while the window shows `seen` for as long as it asked, and a change ends the
    /// wait at once, one made before the wait began included, as it read `seen` before then.
    /// Thirty seconds bound each wait that must not last.
    #[test]
    fn a_wait_ends_on_any_change_past_what_it_saw() {
        const LONG: Duration = Duration::from_secs(30);
        let window = WindowToken(1);
        let log = Arc::new(FrameLog::default());
        log.open(window);
        log.record(window, summary(1, false));
        let seen = log.get(window).unwrap();

        let held = Instant::now();
        log.wait_changed(window, seen, Duration::from_millis(5));
        assert!(
            held.elapsed() >= Duration::from_millis(5),
            "nothing changed"
        );

        let end_on = |change: &dyn Fn(&FrameLog)| {
            let began = Instant::now();
            change(&log);
            log.wait_changed(window, seen, LONG);
            assert!(began.elapsed() < LONG);
        };
        end_on(&|log| log.record(window, summary(2, false)));
        log.record(window, seen.summary);
        end_on(&|log| log.set_hidden(window, true));
        log.set_hidden(window, false);
        end_on(&|log| log.remove(window));

        // The window opened again with the summary it had: a new opening, which no longer holds.
        log.open(window);
        log.record(window, seen.summary);
        let began = Instant::now();
        log.wait_changed(window, seen, LONG);
        assert!(began.elapsed() < LONG, "another opening ends the wait");
        let seen = log.get(window).unwrap();
        let writer = Arc::clone(&log);
        let write = thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            writer.record(window, summary(3, false));
        });
        let began = Instant::now();
        log.wait_changed(window, seen, LONG);
        assert!(
            began.elapsed() < LONG,
            "a write from another thread wakes it"
        );
        write.join().unwrap();
    }
}
