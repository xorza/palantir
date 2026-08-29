//! What a value-scrubbing widget reports about the value it writes
//! through.

use crate::widgets::response::Response;

/// What a gesture-driven numeric widget reports about the value it writes
/// through.
///
/// One type for [`Slider`](crate::Slider) and
/// [`DragValue`](crate::DragValue): both bind a number, both write it
/// across a drag, and both owe the caller the same two signals — so a
/// caller that handles one handles the other, and neither can drift into
/// its own meaning for `changed`.
///
/// [`TextEditResponse`](crate::TextEditResponse) stays separate: a text
/// editor reports focus and submit edges a scrub has no equivalent of.
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
