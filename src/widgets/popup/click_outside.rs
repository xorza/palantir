//! What a click outside a popup does to it.

/// What happens when the user presses outside the popup's body.
///
/// [`Self::Block`] and [`Self::Dismiss`] are the modal pair: each
/// installs a full-surface "click-eater" leaf in the `Popup` layer behind
/// the popup body — outside presses hit the eater (it senses
/// `CLICK | DRAG | SCROLL | PINCH`) and don't propagate to the `Main`
/// tree underneath — and each takes the layer's whole key scope, cutting
/// off every layer below. They differ only in whether the popup widget
/// signals dismissal:
///
/// - [`Self::Block`] — eater consumes the click; no signal (and Esc is
///   ignored). Use for confirm dialogs, stop-the-world prompts.
/// - [`Self::Dismiss`] — an eaten outside-click **or** an Esc press sets
///   `OverlayResponse::dismissed` so the host can flip its open flag. Use for
///   dropdowns, context menus, autocomplete.
/// - [`Self::PassThrough`] — neither capture: no eater, no key-scope
///   claim. Presses and keys outside the body reach `Main` untouched and
///   never signal dismissal. Use for overlays that *annotate* rather than
///   interrupt — toasts, notifications, hover cards — where the host has
///   to stay live underneath.
///
/// The distinction bites hardest for an overlay recorded unconditionally
/// every frame. Under the modal pair that is a permanently dead host: the
/// eater swallows every pointer event and the key claim silences the
/// keyboard, with no interaction able to reach whatever would close it.
/// Under `PassThrough` it is exactly the harmless always-on banner it
/// looks like.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClickOutside {
    /// Swallow the press and stay open. A modal overlay.
    Block,
    /// Swallow the press and close. The default.
    Dismiss,
    /// Let the press reach what is under the overlay, which stays open.
    PassThrough,
}
