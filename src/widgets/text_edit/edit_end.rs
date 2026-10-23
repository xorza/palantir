//! [`EditEnd`]: how a frame ended a text edit.

/// How a frame ended a text edit. A frame ends an edit one way at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditEnd {
    /// Enter in a single-line editor: the accept signal.
    Submitted,
    /// Escape with no selection left to collapse: the cancel signal, which also blurs the
    /// editor.
    Canceled,
}
