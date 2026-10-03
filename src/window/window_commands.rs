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

    /// Enqueue a close for `token`, once. A frame's replayed passes —
    /// warmup, pass A, pass B — each record the same close, and one drain
    /// applies every close in it to the window the token names then.
    pub(crate) fn close(&mut self, token: WindowToken) {
        if !self.closes.contains(&token) {
            self.closes.push(token);
        }
    }

    /// Move every command out of `source` onto the end of `self`, leaving
    /// `source` empty with its buffers — and their capacity — intact.
    pub(crate) fn append(&mut self, source: &mut Self) {
        self.opens.append(&mut source.opens);
        self.closes.append(&mut source.closes);
    }
}

#[cfg(test)]
mod tests {
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_token::WindowToken;

    /// A close recorded by each replayed pass of a frame is one close.
    #[test]
    fn a_close_enqueues_once_per_token() {
        let mut commands = WindowCommands::default();
        for _ in 0..3 {
            commands.close(WindowToken(7));
        }
        commands.close(WindowToken(8));
        assert_eq!(commands.closes, [WindowToken(7), WindowToken(8)]);
    }
}
