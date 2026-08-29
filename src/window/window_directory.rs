//! The set of window identities currently live.

use crate::window::window_token::WindowToken;
use std::cell::RefCell;
use std::rc::Rc;

/// App-global set of live application window identities shared between the
/// host lifecycle and every recorder.
///
/// The recorder half is why this exists at all: a `Ui` answers
/// [`Ui::window_open`](crate::Ui::window_open) and cannot see the host's own
/// window list, which is a winit type. So this is that list projected down
/// to the one fact a recorder needs.
///
/// **A `WindowDriver` is the thing that registers.** Its `build` adds the
/// token and its `Drop` removes it, so the set is "the drivers that exist"
/// by construction rather than by two hosts remembering to say so, and a
/// driver builder dropped without building leaves nothing behind.
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowDirectory {
    tokens: Rc<RefCell<Vec<WindowToken>>>,
}

impl WindowDirectory {
    pub(crate) fn contains(&self, token: WindowToken) -> bool {
        self.tokens.borrow().contains(&token)
    }

    /// Register a newly built driver's token.
    ///
    /// # Panics
    ///
    /// Panics on a token already live. Two drivers under one token would
    /// leave `Ui::window_open` true after the first closed, and the host
    /// routing an app's commands to whichever it scanned first.
    pub(crate) fn add(&self, token: WindowToken) {
        let mut tokens = self.tokens.borrow_mut();
        assert!(
            !tokens.contains(&token),
            "window directory already contains {token:?}"
        );
        tokens.push(token);
    }

    /// Retire a dropped driver's token.
    pub(crate) fn remove(&self, token: WindowToken) {
        let mut tokens = self.tokens.borrow_mut();
        let index = tokens
            .iter()
            .position(|candidate| *candidate == token)
            .expect("a dropped window driver must be in the window directory");
        tokens.swap_remove(index);
    }
}

#[cfg(test)]
mod tests {
    use crate::window::window_directory::WindowDirectory;
    use crate::window::window_token::WindowToken;

    #[test]
    fn directory_clones_observe_the_same_live_windows() {
        let host = WindowDirectory::default();
        let recorder = host.clone();

        host.add(WindowToken(1));
        host.add(WindowToken(2));
        assert!(recorder.contains(WindowToken(1)));
        assert!(recorder.contains(WindowToken(2)));

        host.remove(WindowToken(1));
        assert!(!recorder.contains(WindowToken(1)));
        assert!(recorder.contains(WindowToken(2)));
    }
}
