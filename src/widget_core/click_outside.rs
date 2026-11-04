//! What a click outside an overlay does to it.

/// What a press outside an overlay's body, and Escape, do to an overlay that holds the input
/// of the layers below: its [`Backdrop`](crate::widget::Backdrop) swallows outside presses, and
/// its layer takes the whole key scope. An overlay with no backdrop holds none, so presses and
/// keys reach `Main` and nothing dismisses it, as a toast's do; a [`Popup`](crate::Popup) is one
/// with `click_outside(None)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    /// Swallow the press and stay open, and ignore Escape. A dialog the user must answer.
    Block,
    /// Swallow the press and close as `OverlayClose::Dismissed`, as Escape does. The default.
    Dismiss,
}
