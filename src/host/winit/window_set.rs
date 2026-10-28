//! The live windows a running host drives, and how an event finds one.

use crate::host::winit::window::Window;
use crate::window::window_token::WindowToken;
use std::slice;
use winit::window::WindowId;

/// The host's live windows, in registration order, addressed by winit [`WindowId`] on the event path and by
/// [`WindowToken`] on the app path (linear scans; counts are tiny). Resolution hands back a [`WindowSlot`]
/// rather than a borrow where the caller needs `&mut` on the rest of the host too.
#[derive(Debug, Default)]
pub(super) struct WindowSet {
    windows: Vec<Window>,
}

/// Where a window sits in its [`WindowSet`]; valid only until the set changes, as `close_window` uses
/// `swap_remove`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WindowSlot(usize);

impl WindowSet {
    pub(super) fn slot_of_id(&self, id: WindowId) -> Option<WindowSlot> {
        self.slot_where(|win| win.window.id() == id)
    }

    pub(super) fn slot_of_token(&self, token: WindowToken) -> Option<WindowSlot> {
        self.slot_where(|win| win.driver.token == token)
    }

    fn slot_where(&self, matches: impl Fn(&Window) -> bool) -> Option<WindowSlot> {
        self.windows.iter().position(matches).map(WindowSlot)
    }

    pub(super) fn at(&mut self, slot: WindowSlot) -> &mut Window {
        &mut self.windows[slot.0]
    }

    pub(super) fn by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        let slot = self.slot_of_token(token)?;
        Some(self.at(slot))
    }

    /// Registers `window`; its winit [`WindowId`] must not already be in the set (a reused live id would misroute
    /// events). A release assert: it checks what the platform handed back, and window creation is cold.
    pub(super) fn push(&mut self, window: Window) {
        assert!(
            self.slot_of_id(window.window.id()).is_none(),
            "a window is already registered under this id",
        );
        self.windows.push(window);
    }

    pub(super) fn pop(&mut self) -> Option<Window> {
        self.windows.pop()
    }

    pub(super) fn take(&mut self, token: WindowToken) -> Option<Window> {
        let slot = self.slot_of_token(token)?;
        Some(self.windows.swap_remove(slot.0))
    }

    pub(super) const fn len(&self) -> usize {
        self.windows.len()
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub(super) fn iter(&self) -> slice::Iter<'_, Window> {
        self.windows.iter()
    }

    pub(super) fn iter_mut(&mut self) -> slice::IterMut<'_, Window> {
        self.windows.iter_mut()
    }
}

impl<'a> IntoIterator for &'a WindowSet {
    type Item = &'a Window;
    type IntoIter = slice::Iter<'a, Window>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut WindowSet {
    type Item = &'a mut Window;
    type IntoIter = slice::IterMut<'a, Window>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
