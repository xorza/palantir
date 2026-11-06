//! What a value-writing widget reports about the value it writes through.

use crate::primitives::input::interaction::response::Response;

/// What a widget that writes a bound value reports about it.
///
/// One type for scrubs and discrete picks, so `changed` means one thing; a pick commits at once.
///
/// The [`Response`] answers a different question: a radio's `clicked()` is true on the selected
/// option, a `ComboBox`'s reports the list opening. Read `changed` for the value.
///
/// [`TextEditResponse`](crate::TextEditResponse) stays separate (cancel, submit, focus edges).
#[derive(Clone, Copy, Debug)]
pub struct ValueResponse {
    /// The widget's own [`Response`].
    pub response: Response,
    /// The bound value was written with something other than what the caller passed this frame.
    ///
    /// A level, not an edge: true every frame an uncommitted draft exists; false on a drag pinned at a range end.
    pub changed: bool,
    /// A gesture finished (drag released, edit ended by Enter or focus loss): one undoable edit.
    ///
    /// The finishing frame re-writes the final value, so a caller re-seeding every frame sees it. Dropped if released while disabled.
    pub committed: bool,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widget_core::value_response::ValueResponse;

    impl ValueResponse {
        /// Assert both signals, one assert each, so a failure names the one that broke.
        #[track_caller]
        pub(crate) fn assert_signals(&self, changed: bool, committed: bool, label: &str) {
            assert_eq!(self.changed, changed, "{label}: changed");
            assert_eq!(self.committed, committed, "{label}: committed");
        }
    }
}
