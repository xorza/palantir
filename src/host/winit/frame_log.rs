//! [`FrameLog`]: each open window's last frame, shared between the event loop and its handles.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::host::winit::frame_summary::FrameSummary;
use crate::window::window_token::WindowToken;

/// The event loop writes a window's summary after each of its frames, and whether it is hidden as
/// that changes, and removes both when the window closes; a [`HostHandle`](crate::HostHandle)
/// reads them on any thread, and a waiter wakes on each change. A host has a few windows, so a
/// scan of a short list under one lock is all a read or a write pays.
#[derive(Debug, Default)]
pub(super) struct FrameLog {
    windows: Mutex<Vec<LoggedFrame>>,
    /// Signaled on each write, so a [`Remote`](crate::Remote) waits for a frame without a poll.
    changed: Condvar,
}

#[derive(Clone, Copy, Debug)]
struct LoggedFrame {
    token: WindowToken,
    /// `None` before the window's first frame.
    summary: Option<FrameSummary>,
    hidden: bool,
}

impl FrameLog {
    pub(super) fn record(&self, token: WindowToken, summary: FrameSummary) {
        Self::entry(&mut self.windows.lock().unwrap(), token).summary = Some(summary);
        self.changed.notify_all();
    }

    /// Block until the next write, or for `timeout` at most.
    pub(super) fn wait(&self, timeout: Duration) {
        let windows = self.windows.lock().unwrap();
        drop(self.changed.wait_timeout(windows, timeout).unwrap());
    }

    /// Note whether `token`'s window is hidden, which holds its frames until it shows.
    pub(super) fn set_hidden(&self, token: WindowToken, hidden: bool) {
        Self::entry(&mut self.windows.lock().unwrap(), token).hidden = hidden;
        self.changed.notify_all();
    }

    pub(super) fn get(&self, token: WindowToken) -> Option<FrameSummary> {
        let windows = self.windows.lock().unwrap();
        windows
            .iter()
            .find(|logged| logged.token == token)
            .and_then(|logged| logged.summary)
    }

    pub(super) fn is_hidden(&self, token: WindowToken) -> bool {
        let windows = self.windows.lock().unwrap();
        windows
            .iter()
            .any(|logged| logged.token == token && logged.hidden)
    }

    /// `token`'s entry, made when the window has none yet.
    fn entry(windows: &mut Vec<LoggedFrame>, token: WindowToken) -> &mut LoggedFrame {
        let idx = windows
            .iter()
            .position(|logged| logged.token == token)
            .unwrap_or_else(|| {
                windows.push(LoggedFrame {
                    token,
                    summary: None,
                    hidden: false,
                });
                windows.len() - 1
            });
        &mut windows[idx]
    }

    pub(super) fn remove(&self, token: WindowToken) {
        self.windows
            .lock()
            .unwrap()
            .retain(|logged| logged.token != token);
        self.changed.notify_all();
    }

    /// Forget every window, as the event loop ends and closes them all.
    pub(super) fn clear(&self) {
        self.windows.lock().unwrap().clear();
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::frame_log::FrameLog;
    use crate::host::winit::frame_summary::FrameSummary;
    use crate::window::window_token::WindowToken;
    use std::time::Duration;

    const fn summary(frame_id: u64, repaint_requested: bool) -> FrameSummary {
        FrameSummary {
            frame_id,
            sent_inputs: 0,
            repaint_requested,
            repaint_after: None,
            busy: false,
        }
    }

    /// Each window keeps its own last frame and hidden state: a record or a hide changes only that
    /// window's, a remove forgets only that window, and a clear forgets all.
    #[test]
    fn each_window_keeps_its_last_frame_until_it_closes() {
        let log = FrameLog::default();
        let (a, b) = (WindowToken(1), WindowToken(2));
        assert_eq!(log.get(a), None, "no frame yet");

        log.record(a, summary(1, true));
        log.record(b, summary(1, false));
        let later = FrameSummary {
            repaint_after: Some(Duration::from_millis(530)),
            ..summary(2, false)
        };
        log.record(a, later);
        assert_eq!(log.get(a), Some(later));
        assert_eq!(log.get(b), Some(summary(1, false)));

        // Hiding leaves the last frame as it was, and is each window's own.
        log.set_hidden(a, true);
        assert_eq!(
            (log.get(a), log.is_hidden(a), log.is_hidden(b)),
            (Some(later), true, false)
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
        assert_eq!(log.get(b), Some(summary(1, false)));

        // A window hidden before its first frame has no frame yet.
        let c = WindowToken(3);
        log.set_hidden(c, true);
        assert_eq!((log.get(c), log.is_hidden(c)), (None, true));

        log.record(a, summary(3, false));
        log.clear();
        assert_eq!([log.get(a), log.get(b)], [None, None]);
    }
}
