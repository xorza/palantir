//! One entry of the per-frame keyboard queue: a press, or text the IME
//! committed.

use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::text_chunk::TextChunk;

/// One queued keyboard entry: a press or an IME-committed text chunk,
/// in event-arrival order. Releases
/// (`KeyUp`) aren't surfaced: editors care about presses, and adding
/// a release variant without a consumer would invent state we don't
/// yet need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    /// Logical key pressed.
    Down(KeyPress),
    /// Committed text from typing or an IME composition that just
    /// finalized. Distinct from `Down` because IME / dead-key
    /// composition produces text without a physical keypress, and
    /// because keys like `Enter` produce a `Down` but no text to
    /// insert.
    Text(TextChunk),
}
