//! What a click outside an overlay does to it.

/// What a press outside an overlay's body, and Escape, do.
///
/// `Block` and `Dismiss` are modal: a [`Backdrop`](crate::widget::Backdrop) swallows outside
/// presses, and the overlay's layer takes the whole key scope. `PassThrough` does neither, so a
/// permanently recorded overlay can't lock the host.
///
/// - `Block`: no dismissal signal; Esc is ignored.
/// - `Dismiss`: an outside click or Esc closes it as `OverlayClose::Dismissed`.
/// - `PassThrough`: presses and keys reach `Main`; never dismisses (toasts).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    /// Swallow the press and stay open. A modal overlay.
    Block,
    /// Swallow the press and close. The default.
    Dismiss,
    /// Let the press reach what is under the overlay, which stays open.
    PassThrough,
}
