//! A key's identity, logical or physical.

/// A key identity: the **logical** key ([`KeyPress::key`](crate::KeyPress::key),
/// after layout; Shift+'a' is `Char('A')`) or the layout-independent
/// **physical** key ([`KeyPress::physical`](crate::KeyPress::physical), the
/// unshifted US-QWERTY identity). Named variants exist for keys with no
/// printable character or a platform-noisy one; anything else is
/// [`Key::Other`]. A press's routing meaning is [`KeyClass`](crate::KeyClass).
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
    /// Forward delete, separate from [`Key::Backspace`]; not every keyboard has it.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Return or Enter, main block or numeric pad; named because `Char('\r')` differs by platform.
    Enter,
    /// Tab, with or without Shift. Moves focus between Tab stops unless a scope
    /// takes [`KeyFilter::FOCUS`](crate::KeyFilter), which reads it as an ordinary press.
    Tab,
    /// Escape: the conventional cancel, and what dismisses an overlay.
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
    /// Printable character, post-layout and post-shift; space is `Char(' ')`.
    Char(char),
    /// Any key not covered above, carried so dispatch can ignore it cleanly.
    Other,
}
