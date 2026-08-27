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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Tab,
    Escape,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    /// Printable character, post-layout (post-shift). Space arrives as
    /// `Char(' ')`, not a dedicated variant.
    Char(char),
    /// Any key not covered by the variants above. Carried so dispatch
    /// can ignore it cleanly without translation losing the keypress.
    Other,
}
