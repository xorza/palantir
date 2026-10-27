//! Window lifecycle commands a recorder defers to the host.

use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use std::vec::Drain;

/// One deferred window lifecycle step, applied by [`WinitHost`](crate::WinitHost) in
/// `about_to_wait`.
#[derive(Debug)]
pub(crate) enum WindowCommand {
    Open {
        token: WindowToken,
        config: WindowConfig,
    },
    Close(WindowToken),
}

/// Deferred window lifecycle commands transferred from recorders to the host, in the order the
/// host applies them against its live windows.
///
/// Each token holds at most a close and then an open, so the last call per token wins: a close
/// drops a pending open, and an open after a close recreates a live window. Record passes replay,
/// so a repeated call changes nothing.
#[derive(Debug, Default)]
pub(crate) struct WindowCommands {
    ops: Vec<WindowCommand>,
}

impl WindowCommands {
    /// Open `token` with `config`; a pending open of `token` takes the new config in place.
    pub(crate) fn open(&mut self, token: WindowToken, config: WindowConfig) {
        let pending = self.ops.iter_mut().find_map(|op| match op {
            WindowCommand::Open { token: t, config } if *t == token => Some(config),
            WindowCommand::Open { .. } | WindowCommand::Close(_) => None,
        });
        match pending {
            Some(pending) => *pending = config,
            None => self.ops.push(WindowCommand::Open { token, config }),
        }
    }

    /// Close `token`, dropping a pending open of it: the window it would build is not wanted.
    pub(crate) fn close(&mut self, token: WindowToken) {
        if let Some(idx) = self
            .ops
            .iter()
            .position(|op| matches!(op, WindowCommand::Open { token: t, .. } if *t == token))
        {
            self.ops.remove(idx);
        }
        if !self
            .ops
            .iter()
            .any(|op| matches!(op, WindowCommand::Close(t) if *t == token))
        {
            self.ops.push(WindowCommand::Close(token));
        }
    }

    /// Move `source`'s commands behind these, as if each were called again here; `source` keeps
    /// its capacity.
    pub(crate) fn append(&mut self, source: &mut Self) {
        for op in source.drain() {
            match op {
                WindowCommand::Open { token, config } => self.open(token, config),
                WindowCommand::Close(token) => self.close(token),
            }
        }
    }

    /// The commands in the order to apply them, leaving the buffer empty with its capacity.
    pub(crate) fn drain(&mut self) -> Drain<'_, WindowCommand> {
        self.ops.drain(..)
    }

    pub(crate) fn first(&self) -> Option<&WindowCommand> {
        self.ops.first()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::window::window_commands::{WindowCommand, WindowCommands};
    use crate::window::window_token::WindowToken;

    /// One command as a test compares it: an open shows its config's title.
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum Seen<'a> {
        Open(WindowToken, &'a str),
        Close(WindowToken),
    }

    impl WindowCommands {
        pub(crate) fn seen(&self) -> Vec<Seen<'_>> {
            self.ops
                .iter()
                .map(|op| match op {
                    WindowCommand::Open { token, config } => Seen::Open(*token, &config.title),
                    WindowCommand::Close(token) => Seen::Close(*token),
                })
                .collect()
        }

        pub(crate) const fn capacity(&self) -> usize {
            self.ops.capacity()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::window::window_commands::WindowCommands;
    use crate::window::window_commands::internals::Seen;
    use crate::window::window_config::WindowConfig;
    use crate::window::window_token::WindowToken;

    /// One recorded call.
    #[derive(Clone, Copy, Debug)]
    enum Call {
        Open(u64, &'static str),
        Close(u64),
    }

    fn record(commands: &mut WindowCommands, calls: &[Call]) {
        for call in calls {
            match *call {
                Call::Open(token, title) => {
                    commands.open(WindowToken(token), WindowConfig::new(title));
                }
                Call::Close(token) => commands.close(WindowToken(token)),
            }
        }
    }

    /// The last call per token wins, and a token holds at most a close and then an open. Each
    /// sequence also replays twice, as record passes do, and lands on the same commands; so does a
    /// split of it over two recorders joined by `append`.
    #[test]
    fn the_last_call_per_token_wins_in_order() {
        use Call::{Close, Open};
        let t = WindowToken;
        for (label, calls, want) in [
            ("open", &[Open(7, "a")][..], &[Seen::Open(t(7), "a")][..]),
            (
                "the last config wins",
                &[Open(7, "a"), Open(7, "b")],
                &[Seen::Open(t(7), "b")],
            ),
            (
                "one close per token",
                &[Close(7), Close(7)],
                &[Seen::Close(t(7))],
            ),
            (
                "a close drops the open",
                &[Open(7, "a"), Close(7)],
                &[Seen::Close(t(7))],
            ),
            (
                "an open after a close recreates",
                &[Close(7), Open(7, "a")],
                &[Seen::Close(t(7)), Seen::Open(t(7), "a")],
            ),
            (
                "a close after the recreate keeps one close",
                &[Close(7), Open(7, "a"), Close(7)],
                &[Seen::Close(t(7))],
            ),
            (
                "tokens keep their own order",
                &[Open(7, "a"), Close(8), Open(9, "c"), Close(7)],
                &[Seen::Close(t(8)), Seen::Open(t(9), "c"), Seen::Close(t(7))],
            ),
        ] {
            let mut once = WindowCommands::default();
            record(&mut once, calls);
            assert_eq!(once.seen(), want, "{label}");

            let mut replayed = WindowCommands::default();
            record(&mut replayed, calls);
            record(&mut replayed, calls);
            assert_eq!(replayed.seen(), want, "{label}: replayed");

            for split in 0..=calls.len() {
                let mut host = WindowCommands::default();
                let mut recorder = WindowCommands::default();
                record(&mut host, &calls[..split]);
                record(&mut recorder, &calls[split..]);
                let capacity = recorder.capacity();
                host.append(&mut recorder);
                assert_eq!(host.seen(), want, "{label}: split at {split}");
                assert!(
                    recorder.seen().is_empty(),
                    "{label}: split at {split}, moved"
                );
                assert_eq!(
                    recorder.capacity(),
                    capacity,
                    "{label}: the recorder keeps its buffer"
                );
            }
        }
    }
}
