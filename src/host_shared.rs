//! [`HostShared`] — app-global state the host owns and every window's
//! [`Ui`](crate::ui::Ui) holds a clone of: the live-window set (so
//! `Ui::window_open` answers without each `Ui` mirroring host state) and
//! the debug overlay (so a toggle from any window is seen by all of
//! them). Created once per host and cloned into each `Ui` at construction
//! through [`RenderContext`](crate::renderer::context::RenderContext),
//! the same way the shaper / arena / caches handles are.
//!
//! Single-threaded by construction — the event loop and every window's
//! frame run on one thread — so it's `Rc<RefCell<>>`, not `Arc<Mutex<>>`.
//! Borrows are taken and dropped within a single method call, never held
//! across a frame, so they can't overlap.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use crate::debug_overlay::DebugOverlayConfig;
use crate::window::WindowToken;

/// Cheap-to-clone handle to the one shared host state. Every window's
/// `Ui` and the host hold a clone; all point at the same cell.
#[derive(Clone, Debug, Default)]
pub(crate) struct HostShared(Rc<RefCell<Inner>>);

#[derive(Debug, Default)]
struct Inner {
    /// Tokens of every currently-live window. The host rewrites it on
    /// open/close; `Ui::window_open` reads it. Retained Vec — capacity is
    /// reused, so the steady state is alloc-free.
    open_windows: Vec<WindowToken>,
    /// App-global debug overlay. The host reads it to drive the backend's
    /// overlay passes; user code toggles it via `Ui::debug_overlay_mut`.
    debug_overlay: DebugOverlayConfig,
    /// Set whenever `debug_overlay` is handed out mutably; the host takes
    /// it each tick to know a window toggled the overlay and idle windows
    /// need repainting. Lives here, with the data, so the host needs no
    /// separate "last seen" snapshot.
    overlay_dirty: bool,
}

impl HostShared {
    pub(crate) fn window_open(&self, token: WindowToken) -> bool {
        self.0.borrow().open_windows.contains(&token)
    }

    /// Replace the live-window set (host-side, on open/close). Reuses the
    /// Vec's capacity.
    pub(crate) fn set_open_windows(&self, tokens: impl IntoIterator<Item = WindowToken>) {
        let mut inner = self.0.borrow_mut();
        inner.open_windows.clear();
        inner.open_windows.extend(tokens);
    }

    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.0.borrow().debug_overlay
    }

    /// Mutable borrow of the app-global overlay; the guard derefs to
    /// `&mut DebugOverlayConfig`. Writes land globally at once, and the
    /// borrow flags the overlay dirty so the host repaints idle windows
    /// (otherwise damage-`Skip`, they'd never show the change).
    pub(crate) fn debug_overlay_mut(&self) -> RefMut<'_, DebugOverlayConfig> {
        let mut inner = self.0.borrow_mut();
        inner.overlay_dirty = true;
        RefMut::map(inner, |inner| &mut inner.debug_overlay)
    }

    /// Take the "overlay toggled since last check" flag. The host calls
    /// this each tick; `true` ⇒ repaint every window. Flagged on any
    /// mutable borrow rather than on an actual value change, but the only
    /// caller is the toggle path, so in practice it fires exactly on a
    /// real change.
    pub(crate) fn take_overlay_dirty(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().overlay_dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_windows_round_trip_is_shared_across_clones() {
        let a = HostShared::default();
        let b = a.clone();
        assert!(!a.window_open(WindowToken(1)));
        a.set_open_windows([WindowToken(1), WindowToken(2)]);
        // The clone points at the same cell, so it sees the write.
        assert!(b.window_open(WindowToken(1)));
        assert!(b.window_open(WindowToken(2)));
        assert!(!b.window_open(WindowToken(3)));
        // A rewrite replaces the set rather than appending.
        a.set_open_windows([WindowToken(3)]);
        assert!(!b.window_open(WindowToken(1)));
        assert!(b.window_open(WindowToken(3)));
    }

    #[test]
    fn overlay_mut_flags_dirty_then_take_clears_it() {
        let h = HostShared::default();
        assert!(!h.take_overlay_dirty(), "fresh state is clean");
        // A mutable borrow flags dirty — even though the value matched the
        // default, the borrow alone trips it (the documented behavior).
        h.debug_overlay_mut().damage_rect = true;
        assert!(h.take_overlay_dirty(), "mutable borrow flags dirty");
        assert!(!h.take_overlay_dirty(), "take clears the flag");
        // The write is observable through the read path.
        assert!(h.debug_overlay().damage_rect);
    }

    #[test]
    fn overlay_writes_and_dirty_flag_are_shared_across_clones() {
        let a = HostShared::default();
        let b = a.clone();
        a.debug_overlay_mut().frame_stats = true;
        // Either handle observes + clears the one shared dirty flag.
        assert!(b.take_overlay_dirty());
        assert!(!a.take_overlay_dirty());
        assert!(b.debug_overlay().frame_stats);
    }
}
