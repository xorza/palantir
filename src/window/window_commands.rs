//! Window lifecycle commands a recorder defers to the host.

use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

/// A window-open request enqueued by
/// [`Ui::open_window`](crate::Ui::open_window), drained by
/// [`WinitHost`](crate::WinitHost) in `about_to_wait` once it holds
/// `&ActiveEventLoop`.
#[derive(Debug)]
pub(crate) struct PendingWindow {
    pub(crate) token: WindowToken,
    pub(crate) config: WindowConfig,
}

/// Deferred window lifecycle commands transferred from recorders to the host.
#[derive(Debug, Default)]
pub(crate) struct WindowCommands {
    pub(crate) opens: Vec<PendingWindow>,
    pub(crate) closes: Vec<WindowToken>,
}

impl WindowCommands {
    /// Enqueue an open for `token`, or re-configure the one already
    /// enqueued for it.
    ///
    /// Deduplicated by token because a token addresses one window: two
    /// opens in a frame are the same window described twice, and the
    /// later description is the one the app meant. Without this the host
    /// would open two windows the app can only address one of.
    pub(crate) fn open(&mut self, token: WindowToken, config: WindowConfig) {
        match self.opens.iter_mut().find(|p| p.token == token) {
            Some(pending) => pending.config = config,
            None => self.opens.push(PendingWindow { token, config }),
        }
    }

    /// Enqueue a close for `token`. Not deduplicated: closing a window
    /// twice is what a host already ignores, and the second close of a
    /// re-opened token is a different window.
    pub(crate) fn close(&mut self, token: WindowToken) {
        self.closes.push(token);
    }

    /// Move every command out of `source` onto the end of `self`, leaving
    /// `source` empty with its buffers — and their capacity — intact.
    pub(crate) fn append(&mut self, source: &mut Self) {
        self.opens.append(&mut source.opens);
        self.closes.append(&mut source.closes);
    }
}
