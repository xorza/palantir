//! What a value-writing widget reports about the value it writes through.

use crate::widget_core::response::Response;

/// What a widget that writes a bound value reports about it.
///
/// One type for every such widget — the scrubs ([`Slider`](crate::Slider),
/// [`DragValue`](crate::DragValue), the [`Splitter`](crate::Splitter) and
/// the colour widgets) and the discrete picks ([`Checkbox`](crate::Checkbox),
/// [`Switch`](crate::Switch), [`RadioButton`](crate::RadioButton),
/// [`ComboBox`](crate::ComboBox)) — so a caller that handles one handles
/// the rest, and none can drift into its own meaning for `changed`.
///
/// A discrete pick has no draft: the click that makes it writes it, so it
/// commits at once and `committed == changed`.
///
/// The [`Response`] is the widget's own, and it answers a different
/// question than `changed`. A radio latches, so its `clicked()` is true on
/// the option already selected; a `ComboBox` writes from a row inside its
/// dropdown, so its `clicked()` reports that the list opened; a click on a
/// disabled checkbox writes nothing. Read `changed` for the value.
///
/// [`TextEditResponse`](crate::TextEditResponse) stays separate: a text
/// editor reports cancel, submit and focus edges a scrub has no
/// equivalent of. Its `changed` and `committed` mean what these do.
#[derive(Debug)]
pub struct ValueResponse<'a> {
    /// The widget's pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// The bound value was written with a value differing from what the
    /// caller passed in this frame.
    ///
    /// A **level**, not a per-input edge: under the commit-deferring
    /// pattern (re-seed from canonical every frame) it is true on every
    /// frame an uncommitted draft exists, and false on a drag pinned at
    /// an end of the range. Live-preview callers apply the value on this.
    pub changed: bool,
    /// A gesture finished this frame and the bound value holds its final
    /// result: the drag released, or edit mode ended (Enter / focus
    /// lost). One gesture, one undoable edit.
    ///
    /// The finishing frame **re-writes** that value, so a caller that
    /// ignores `changed`, re-seeds the bound number from its own
    /// canonical copy every frame, and adopts it only here still observes
    /// what the gesture landed on. Released while disabled, the gesture
    /// is dropped instead.
    pub committed: bool,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widget_core::value_response::ValueResponse;

    /// A [`ValueResponse`]'s two signals, copied out of the record pass
    /// whose `ui` borrow the response holds.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct ValueEdges {
        pub(crate) changed: bool,
        pub(crate) committed: bool,
    }

    impl ValueResponse<'_> {
        pub(crate) const fn edges(&self) -> ValueEdges {
            ValueEdges {
                changed: self.changed,
                committed: self.committed,
            }
        }
    }
}
