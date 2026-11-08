//! The live windows a running host drives, and how an event finds one.

use crate::host::winit::window::{FramePresent, Window};
use crate::window::window_token::WindowToken;
use std::slice;
use std::time::Instant;
use winit::window::WindowId;

/// What a window set's pacing reads and moves of each window: its next frame, the instant before
/// which it starts none, and the redraw a frame due now asks the platform for. A trait so the
/// pacing runs on a test's windows and clock, with no winit window or event loop. Refused: the next
/// frames moved into a column of the set, which every window method that asks for a frame would
/// have had to hand back.
pub(super) trait Paced {
    fn next(&self) -> FramePresent;
    fn set_next(&mut self, next: FramePresent);
    fn request_redraw(&self);
    fn earliest_redraw_at(&self) -> Option<Instant>;
}

/// The host's live windows, in registration order, addressed by winit [`WindowId`] on the event path and by
/// [`WindowToken`] on the app path (linear scans; counts are tiny). Resolution hands back a [`WindowSlot`]
/// rather than a borrow where the caller needs `&mut` on the rest of the host too.
#[derive(Debug)]
pub(super) struct WindowSet<W = Window> {
    windows: Vec<W>,
}

impl<W> Default for WindowSet<W> {
    fn default() -> Self {
        Self {
            windows: Vec::new(),
        }
    }
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
        self.slot_where(|win| win.frames.token() == token)
    }

    fn slot_where(&self, matches: impl Fn(&Window) -> bool) -> Option<WindowSlot> {
        self.windows.iter().position(matches).map(WindowSlot)
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

    pub(super) fn take(&mut self, token: WindowToken) -> Option<Window> {
        let slot = self.slot_of_token(token)?;
        Some(self.windows.swap_remove(slot.0))
    }
}

impl<W: Paced> WindowSet<W> {
    pub(super) fn at(&mut self, slot: WindowSlot) -> &mut W {
        &mut self.windows[slot.0]
    }

    /// Run `frame` on the window at `slot`, which sets that window's next frame and answers
    /// whether it ran the app, or `None` for a frame it skipped. When the frame was the window's
    /// own, by the next frame it was due as at `started`, read before `frame` moves it, and ran the
    /// app, every other window follows it ([`FramePresent::leads`]) as of `clock` after it.
    pub(super) fn run(
        &mut self,
        slot: WindowSlot,
        started: Instant,
        frame: impl FnOnce(&mut W) -> Option<bool>,
        clock: impl FnOnce() -> Instant,
    ) {
        let win = &mut self.windows[slot.0];
        let due = win.next();
        let Some(ran_app) = frame(win) else {
            return;
        };
        if due.leads(started, ran_app) {
            let now = clock();
            for (idx, win) in self.windows.iter_mut().enumerate() {
                if idx != slot.0 {
                    win.set_next(win.next().followed(now));
                }
            }
        }
    }

    /// Every window follows a change made outside the windows' frames.
    pub(super) fn follow_all(&mut self, now: Instant) {
        for win in &mut self.windows {
            win.set_next(win.next().followed(now));
        }
    }

    /// Every window's next frame at `at`, as a device recovery waits out its backoff: no wake of
    /// a window's own replaces it.
    pub(super) fn hold_until(&mut self, at: Instant) {
        for win in &mut self.windows {
            win.set_next(FramePresent::At(at));
        }
    }

    /// Ask a redraw of every window due at `now`, and return the nearest deadline ahead. A
    /// window's next frame waits out its reduced-power interval first, whatever made it due.
    pub(super) fn wake(&self, now: Instant) -> Option<Instant> {
        earliest_wake(
            self.windows.iter().map(|win| {
                (
                    win,
                    win.next().held_until(win.earliest_redraw_at()).resolve(now),
                )
            }),
            Paced::request_redraw,
        )
    }

    pub(super) fn pop(&mut self) -> Option<W> {
        self.windows.pop()
    }

    pub(super) const fn len(&self) -> usize {
        self.windows.len()
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    pub(super) fn iter(&self) -> slice::Iter<'_, W> {
        self.windows.iter()
    }

    pub(super) fn iter_mut(&mut self) -> slice::IterMut<'_, W> {
        self.windows.iter_mut()
    }
}

/// The nearest future deadline among `presents`; windows wanting a frame now go to `redraw`.
fn earliest_wake<W>(
    presents: impl IntoIterator<Item = (W, FramePresent)>,
    mut redraw: impl FnMut(W),
) -> Option<Instant> {
    let mut earliest: Option<Instant> = None;
    for (window, present) in presents {
        match present {
            FramePresent::Immediate | FramePresent::Follow => redraw(window),
            FramePresent::At(at) => earliest = Some(earliest.map_or(at, |best| best.min(at))),
            FramePresent::Idle => {}
        }
    }
    earliest
}

impl<'a, W: Paced> IntoIterator for &'a WindowSet<W> {
    type Item = &'a W;
    type IntoIter = slice::Iter<'a, W>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, W: Paced> IntoIterator for &'a mut WindowSet<W> {
    type Item = &'a mut W;
    type IntoIter = slice::IterMut<'a, W>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests;
