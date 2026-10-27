//! [`FrameLog`]: each open window's last frame, shared between the event loop and its handles.

use std::sync::Mutex;

use crate::host::winit::frame_summary::FrameSummary;
use crate::window::window_token::WindowToken;

/// The event loop writes a window's summary after each of its frames and removes it when the
/// window closes; a [`HostHandle`](crate::HostHandle) reads it on any thread. A host has a few
/// windows, so a scan of a short list under one lock is all a read or a write pays.
#[derive(Debug, Default)]
pub(super) struct FrameLog {
    windows: Mutex<Vec<LoggedFrame>>,
}

#[derive(Clone, Copy, Debug)]
struct LoggedFrame {
    token: WindowToken,
    summary: FrameSummary,
}

impl FrameLog {
    pub(super) fn record(&self, token: WindowToken, summary: FrameSummary) {
        let mut windows = self.windows.lock().unwrap();
        match windows.iter_mut().find(|logged| logged.token == token) {
            Some(logged) => logged.summary = summary,
            None => windows.push(LoggedFrame { token, summary }),
        }
    }

    pub(super) fn get(&self, token: WindowToken) -> Option<FrameSummary> {
        let windows = self.windows.lock().unwrap();
        windows
            .iter()
            .find(|logged| logged.token == token)
            .map(|logged| logged.summary)
    }

    pub(super) fn remove(&self, token: WindowToken) {
        self.windows
            .lock()
            .unwrap()
            .retain(|logged| logged.token != token);
    }

    /// Forget every window, as the event loop ends and closes them all.
    pub(super) fn clear(&self) {
        self.windows.lock().unwrap().clear();
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
        }
    }

    /// Each window keeps its own last frame: a record replaces only that window's, a remove
    /// forgets only that window, and a clear forgets all.
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

        log.remove(a);
        assert_eq!(log.get(a), None);
        assert_eq!(log.get(b), Some(summary(1, false)));

        log.record(a, summary(3, false));
        log.clear();
        assert_eq!([log.get(a), log.get(b)], [None, None]);
    }
}
