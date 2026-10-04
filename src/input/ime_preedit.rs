//! The input method's uncommitted text, as a widget reads it.

use crate::common::span::Span;

/// What an input method shows of a composition it has not committed:
/// the text so far, and its cursor in it. A host sends it in
/// [`InputEvent::ImePreedit`](crate::InputEvent::ImePreedit), and the
/// widget that asked for IME text reads it back with
/// [`Ui::ime_preedit`](crate::Ui::ime_preedit): it draws the text at its
/// caret, and the bound value changes only when the composition commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImePreedit<'a> {
    /// The composition so far. Empty only in an event, where it ends the
    /// composition; never empty from `Ui::ime_preedit`.
    pub text: &'a str,
    /// The input method's cursor or selection in `text`, as byte offsets
    /// on character boundaries, when it reports one. An event whose cursor
    /// is past the text or inside a character is refused.
    pub cursor: Option<Span>,
}
