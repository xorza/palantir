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
    /// Move every command out of `source` onto the end of `self`, leaving
    /// `source` empty with its buffers — and their capacity — intact.
    pub(crate) fn append(&mut self, source: &mut Self) {
        self.opens.append(&mut source.opens);
        self.closes.append(&mut source.closes);
    }
}
