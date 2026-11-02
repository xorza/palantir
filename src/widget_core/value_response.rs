//! What a value-writing widget reports about the value it writes through.

use crate::widget_core::response::Response;

/// What a widget that writes a bound value reports about it.
///
/// One type for scrubs and discrete picks, so `changed` means one thing; a pick commits at once.
///
/// The [`Response`] answers a different question: a radio's `clicked()` is true on the selected
/// option, a `ComboBox`'s reports the list opening. Read `changed` for the value.
///
/// [`TextEditResponse`](crate::TextEditResponse) stays separate (cancel, submit, focus edges).
#[derive(Debug)]
pub struct ValueResponse<'a> {
    /// The widget's own [`Response`].
    pub response: Response<'a>,
    /// The bound value was written with something other than what the caller passed this frame.
    ///
    /// A level, not an edge: true every frame an uncommitted draft exists; false on a drag pinned at a range end.
    pub changed: bool,
    /// A gesture finished (drag released, edit ended by Enter or focus loss): one undoable edit.
    ///
    /// The finishing frame re-writes the final value, so a caller re-seeding every frame sees it. Dropped if released while disabled.
    pub committed: bool,
}

/// A [`ValueResponse`]'s two signals without the [`Response`] that borrows the `Ui`: what a
/// composite widget gathers from the controls inside it, and a test reads out of a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ValueEdges {
    pub(crate) changed: bool,
    pub(crate) committed: bool,
}

impl ValueResponse<'_> {
    /// The two signals, copied out so the `ui` borrow can end.
    pub(crate) const fn edges(&self) -> ValueEdges {
        ValueEdges {
            changed: self.changed,
            committed: self.committed,
        }
    }
}
