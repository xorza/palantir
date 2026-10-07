//! What a click outside a popup does to it.

/// What a press outside the popup body does.
///
/// `Block` and `Dismiss` are modal: a full-surface click-eater in the `Popup`
/// layer swallows outside presses, and the layer takes the whole key scope.
/// `PassThrough` does neither, so a permanently recorded overlay can't lock
/// the host.
///
/// - `Block`: no dismissal signal; Esc is ignored (confirm dialogs).
/// - `Dismiss`: an outside click or Esc sets `OverlayResponse::dismissed`.
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
