//! The set of window identities currently live.

use crate::window::window_token::WindowToken;
use std::cell::RefCell;
use std::rc::Rc;

/// App-global set of live window identities shared by the host lifecycle and every recorder. A `Ui` answers [`Ui::is_window_open`](crate::Ui::is_window_open) but can't see the host's winit window list, so this projects it to the one fact a recorder needs.
///
/// **A `WindowDriver` registers**: `build` adds its token and `Drop` removes it, so the set is "the drivers that exist" by construction, and an unbuilt builder leaves nothing.
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
    /// Panics on a token already live: `Ui::is_window_open` would stay true after the first closed, and commands would route to whichever driver was scanned first.
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
mod tests;
