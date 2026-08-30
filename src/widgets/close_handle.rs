//! The close request an overlay hands to its body.

use std::cell::Cell;

/// The dismissal request handed to an overlay's body closure, so content
/// widgets can close the [`Popup`](crate::Popup) or
/// [`Modal`](crate::Modal) they are inside.
///
/// Lives on the stack for the duration of one `show` call — no ambient
/// `Ui` state, no nested-overlay signal leak.
///
/// Carries only the close request. Reading input needs no handle: a body
/// records *inside* the overlay's layer and its scope, so plain
/// [`Ui::key_pressed`](crate::Ui::key_pressed) /
/// [`Ui::keyboard_events`](crate::Ui::keyboard_events) already answer as
/// the overlay. Owner-scoped forwarders on this handle would only
/// re-arrange what the scope already provides.
#[derive(Debug, Default)]
pub struct CloseHandle {
    requested: Cell<bool>,
}

impl CloseHandle {
    /// Ask the enclosing overlay to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
    }

    pub(crate) fn requested(&self) -> bool {
        self.requested.get()
    }
}
