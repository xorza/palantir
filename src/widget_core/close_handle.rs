//! The close request an overlay hands to its body.

use std::cell::Cell;

/// The dismissal request handed to an overlay's body, so content that is not a row can close the
/// [`Popup`](crate::Popup) or [`Modal`](crate::Modal) it is inside, as a dialog's button does; an
/// activated [`MenuItem`](crate::MenuItem) closes its overlay without one.
/// [`OverlayScope::record`](crate::widget::OverlayScope::record) makes one for each body it
/// records, and folds it into the turn's close; input needs no handle since a body records
/// inside the overlay's scope.
#[derive(Debug, Default)]
pub struct CloseHandle {
    requested: Cell<bool>,
}

impl CloseHandle {
    /// Ask the enclosing overlay to dismiss.
    pub fn close(&self) {
        self.requested.set(true);
    }

    /// Whether anything asked to dismiss; read by the overlay after its body ran.
    pub const fn requested(&self) -> bool {
        self.requested.get()
    }
}
