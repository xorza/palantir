//! The set of window identities currently live.

use crate::window::window_token::WindowToken;
use std::cell::RefCell;
use std::rc::Rc;

/// App-global set of live application window identities shared between the
/// host lifecycle and every recorder.
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowDirectory {
    tokens: Rc<RefCell<Vec<WindowToken>>>,
}

impl WindowDirectory {
    pub(crate) fn contains(&self, token: WindowToken) -> bool {
        self.tokens.borrow().contains(&token)
    }

    pub(crate) fn set_live(&self, token: WindowToken, live: bool) {
        let mut tokens = self.tokens.borrow_mut();
        let index = tokens.iter().position(|candidate| *candidate == token);
        if live {
            assert!(
                index.is_none(),
                "window directory already contains {token:?}"
            );
            tokens.push(token);
        } else {
            tokens.swap_remove(index.expect("removed window must exist in the window directory"));
        }
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

        host.set_live(WindowToken(1), true);
        host.set_live(WindowToken(2), true);
        assert!(recorder.contains(WindowToken(1)));
        assert!(recorder.contains(WindowToken(2)));

        host.set_live(WindowToken(1), false);
        assert!(!recorder.contains(WindowToken(1)));
        assert!(recorder.contains(WindowToken(2)));
    }
}
