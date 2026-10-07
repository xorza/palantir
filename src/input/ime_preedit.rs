//! The input method's uncommitted text, as a widget reads it.

use crate::common::span::Span;

/// What an input method shows of an uncommitted composition. A host sends it in [`InputEvent::ImePreedit`](crate::InputEvent::ImePreedit); the asking widget reads it via [`Ui::ime_preedit`](crate::Ui::ime_preedit), and the bound value changes only on commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImePreedit<'a> {
    /// The composition so far; empty only in an event, where it ends the composition.
    pub text: &'a str,
    /// The input method's cursor in `text`, as byte offsets on character boundaries. An event with an out-of-range or mid-character cursor is refused.
    pub cursor: Option<Span>,
}
