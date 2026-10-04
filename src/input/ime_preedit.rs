//! The input method's uncommitted text, as a widget reads it.

use std::ops::Range;

/// What an input method shows of a composition it has not committed:
/// the text so far, and its cursor in it. Read with
/// [`Ui::ime_preedit`](crate::Ui::ime_preedit) by the widget that asked
/// for IME text; it draws the text at its caret, and the bound value
/// changes only when the composition commits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImePreedit<'a> {
    /// The composition so far. Never empty: an empty one has ended.
    pub text: &'a str,
    /// The input method's cursor or selection in `text`, as byte offsets
    /// on character boundaries, when it reports one.
    pub cursor: Option<Range<usize>>,
}
