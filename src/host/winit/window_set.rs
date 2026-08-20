//! The live windows a running host drives, and how an event finds one.

use winit::window::WindowId;

use crate::host::winit::window::Window;
use crate::window::window_token::WindowToken;

/// The host's live windows, in registration order.
///
/// **Two keys, one place that knows how to match on either.** A window is
/// addressed by its winit [`WindowId`] on the event path and by its
/// [`WindowToken`] on the app path, and both used to be linear scans with
/// the predicate spelled out at the call site — so a scan could be written
/// a fifth time, or written twice for one event. Lookups stay linear
/// because window counts are tiny; what this owns is that they are spelled
/// once.
///
/// Resolution hands back a [`WindowSlot`] rather than a borrow where the
/// caller needs `&mut` on the rest of the host too, which is what lets one
/// event resolve its window once and then act on it.
#[derive(Debug, Default)]
pub(super) struct WindowSet {
    windows: Vec<Window>,
}

/// Where a window sits in its [`WindowSet`].
///
/// **Valid only until the set changes.** Every use resolves and consumes
/// one inside a single event or command, which is why this is an index
/// rather than a handle with a lifetime: `close_window` uses `swap_remove`,
/// so a slot held across one would name a different window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WindowSlot(usize);

impl WindowSet {
    /// Slot of the window winit reports events for as `id`.
    pub(super) fn slot_of_id(&self, id: WindowId) -> Option<WindowSlot> {
        self.slot_where(|win| win.window.id() == id)
    }

    /// Slot of the window the app addresses as `token`.
    pub(super) fn slot_of_token(&self, token: WindowToken) -> Option<WindowSlot> {
        self.slot_where(|win| win.driver.token == token)
    }

    fn slot_where(&self, matches: impl Fn(&Window) -> bool) -> Option<WindowSlot> {
        self.windows.iter().position(matches).map(WindowSlot)
    }

    pub(super) fn at(&mut self, slot: WindowSlot) -> &mut Window {
        &mut self.windows[slot.0]
    }

    /// The window the app addresses as `token`, resolved and borrowed in
    /// one step — for a caller that needs nothing else off the host.
    pub(super) fn by_token(&mut self, token: WindowToken) -> Option<&mut Window> {
        let slot = self.slot_of_token(token)?;
        Some(self.at(slot))
    }

    /// Register `window`, which must not already be in the set under
    /// either key — winit reusing a live `WindowId`, or `spawn_window`
    /// letting a duplicate token through, would both give one window two
    /// entries and route half its events to the wrong one.
    ///
    /// A release assert, not a debug one: what it checks is what the
    /// platform handed back rather than arithmetic of ours, and window
    /// creation is cold enough to pay two scans of a handful of entries
    /// for it.
    pub(super) fn push(&mut self, window: Window) {
        assert!(
            self.slot_of_id(window.window.id()).is_none()
                && self.slot_of_token(window.driver.token).is_none(),
            "a window is already registered under this id or token",
        );
        self.windows.push(window);
    }

    /// Remove the window holding `token` and hand it back, or `None` if
    /// none does.
    pub(super) fn take(&mut self, token: WindowToken) -> Option<Window> {
        let slot = self.slot_of_token(token)?;
        Some(self.windows.swap_remove(slot.0))
    }

    pub(super) fn len(&self) -> usize {
        self.windows.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &Window> {
        self.windows.iter()
    }

    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Window> {
        self.windows.iter_mut()
    }
}
