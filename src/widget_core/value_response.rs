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
    /// A gesture finished with the value changed (a release that leaves it apart from where the
    /// press found it, a key that moved it, an edit ended by Enter or focus loss): one undoable
    /// edit. A commit is of a change, as HTML's `change` event is: a click that moves nothing and
    /// a key at a bound commit nothing.
    ///
    /// The finishing frame re-writes the final value, so a caller re-seeding every frame sees it. Dropped if released while disabled.
    pub committed: bool,
}

impl ValueResponse {
    /// A pick, which writes and commits at once: a toggle, a radio, a list's choice.
    pub(crate) const fn pick(response: Response, changed: bool) -> Self {
        Self {
            response,
            changed,
            committed: changed,
        }
    }

    /// A scrub: the pointer wrote the value this frame (`pointed`) or a key moved it (`keyed`),
    /// and `release_moved` says the frame is the press's release and leaves the value apart from
    /// where the press found it. A key commits at once, a pointer gesture at its release.
    pub(crate) const fn gesture(
        response: Response,
        pointed: bool,
        keyed: bool,
        release_moved: bool,
    ) -> Self {
        Self {
            response,
            changed: pointed || keyed,
            committed: !response.disabled && (keyed || release_moved),
        }
    }
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
