//! A key's identity — used both as what the layout produced and as the
//! physical position it came from.

/// A key identity. Used two ways on [`KeyPress`](crate::KeyPress): as the
/// **logical** key ([`KeyPress::key`](crate::KeyPress::key)) — after the
/// keyboard layout has been applied, so Shift+'a'
/// arrives as `Char('A')`, same convention as winit — and as the
/// **layout-independent physical** key ([`KeyPress::physical`](crate::KeyPress::physical)), the US-QWERTY
/// identity of the pressed position (always the unshifted form, e.g. `Char('z')`
/// for the Z position).
///
/// `Char` covers letters, digits, and punctuation in a single arm; the
/// named variants only exist for keys that *don't* produce a printable
/// character (or whose printable form is platform-noisy, like `Enter →
/// '\r'`). Anything not covered collapses to [`Key::Other`] so callers
/// can still see "a key happened" without needing every esoteric key
/// modeled.
///
/// What a press *means* to input routing is [`KeyClass`](crate::KeyClass),
/// not this type: the arrows, Home, End, the paging keys and Tab are
/// `Motion`, Backspace and Delete are `Edit`, Escape is its own class, and
/// the function keys are `Accel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Backspace, which macOS keyboards label "delete".
    Backspace,
    /// Forward delete — a separate key from [`Key::Backspace`], and not
    /// every keyboard has it.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Return / Enter, from either the main block or the numeric pad.
    /// Named rather than `Char('\r')`, whose printable form differs by
    /// platform.
    Enter,
    /// Tab. This crate binds no focus traversal to it — it arrives as an
    /// ordinary press for a widget or a scope to claim.
    Tab,
    /// Escape — the conventional cancel, and what dismisses an overlay.
    Escape,
    /// Function key 1.
    F1,
    /// Function key 2.
    F2,
    /// Function key 3.
    F3,
    /// Function key 4.
    F4,
    /// Function key 5.
    F5,
    /// Function key 6.
    F6,
    /// Function key 7.
    F7,
    /// Function key 8.
    F8,
    /// Function key 9.
    F9,
    /// Function key 10.
    F10,
    /// Function key 11.
    F11,
    /// Function key 12.
    F12,
    /// Printable character, post-layout (post-shift). Space arrives as
    /// `Char(' ')`, not a dedicated variant.
    Char(char),
    /// Any key not covered by the variants above. Carried so dispatch
    /// can ignore it cleanly without translation losing the keypress.
    Other,
}
