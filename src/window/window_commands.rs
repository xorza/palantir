//! Window lifecycle commands a recorder defers to the host.

use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

/// A window-open request, drained by [`WinitHost`](crate::WinitHost) in `about_to_wait`.
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
    pub(crate) fn open(&mut self, token: WindowToken, config: WindowConfig) {
        match self.opens.iter_mut().find(|p| p.token == token) {
            Some(pending) => pending.config = config,
            None => self.opens.push(PendingWindow { token, config }),
        }
    }

    pub(crate) fn close(&mut self, token: WindowToken) {
        if !self.closes.contains(&token) {
            self.closes.push(token);
        }
    }

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
