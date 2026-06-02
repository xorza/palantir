//! Keyboard event vocabulary. The shape was sized for `TextEdit`'s
//! step-1 needs: a small `Key` enum covering navigation/editing keys
//! plus printable characters, a `Modifiers` struct, and an inline
//! `TextChunk` so [`crate::input::InputEvent`] stays `Copy`.
//!
//! Translation lives in [`crate::input::InputEvent::from_winit`].
//! Consumers: `TextEdit`, the [`crate::Shortcut`] matcher, and global
//! [`crate::input::subscriptions::KeyboardSense`] subscribers, fed from
//! the per-frame keyboard-event queue drained during the frame.

use crate::common::platform::{PLATFORM, Platform};

/// Logical key, after the keyboard layout has been applied. Shift+'a'
/// arrives as `Char('A')`, not `Char('a')` — same convention as winit.
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

/// Modifier-key state. Sent as a standalone [`InputEvent::ModifiersChanged`]
/// whenever the held set changes; widgets read the latest snapshot from
/// [`InputState`] (wiring in step 2).
///
/// `ctrl` is the **primary command modifier**, already normalized at
/// the input boundary ([`modifiers_from_winit`]): it's the Cmd (⌘)
/// key on macOS and the physical Ctrl key on Windows/Linux. Consumers
/// never disambiguate platforms for normal shortcuts — there's one
/// command bit.
///
/// `mac_ctrl` is the **raw macOS Control key**, surfaced separately
/// for the rare Mac-specific binding (Ctrl-click → context menu,
/// emacs-style Ctrl-A in a field). It's only ever set on macOS; on
/// Windows/Linux the physical Ctrl *is* the primary, so it lands in
/// `ctrl` and `mac_ctrl` stays `false`. Most code should ignore it.
///
/// [`InputEvent::ModifiersChanged`]: crate::input::InputEvent::ModifiersChanged
/// [`InputState`]: crate::input::InputState
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub mac_ctrl: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        mac_ctrl: false,
    };

    /// True if any command modifier (primary ctrl, alt, or raw macOS
    /// Control) is held — the canonical "this is a shortcut, not text"
    /// predicate. Shift alone doesn't count (shift+letter is just the
    /// capitalized letter).
    pub const fn any_command(self) -> bool {
        self.ctrl || self.alt || self.mac_ctrl
    }
}

/// Inline UTF-8 byte buffer carried by [`InputEvent::Text`]. Sized for
/// the common case (a single grapheme cluster ≤ 15 bytes); longer IME
/// commits split across multiple events at the translation boundary.
/// Inline storage keeps `InputEvent: Copy`.
///
/// [`InputEvent::Text`]: crate::input::InputEvent::Text
#[derive(Clone, Copy)]
pub struct TextChunk {
    bytes: [u8; Self::INLINE_CAP],
    len: u8,
}

impl TextChunk {
    pub const INLINE_CAP: usize = 15;

    /// Build a chunk from `s`. Returns `None` if `s` exceeds the inline
    /// capacity. Callers translating from winit should split at
    /// grapheme boundaries before calling — never split mid-codepoint.
    pub fn new(s: &str) -> Option<Self> {
        if s.len() > Self::INLINE_CAP {
            return None;
        }
        let mut bytes = [0u8; Self::INLINE_CAP];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Some(Self {
            bytes,
            len: s.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: `new` only stores valid UTF-8 from a `&str`,
        // and `len` always reflects the byte count written.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len as usize]) }
    }
}

impl std::fmt::Debug for TextChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextChunk({:?})", self.as_str())
    }
}

impl PartialEq for TextChunk {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for TextChunk {}

/// Payload of [`KeyboardEvent::Down`] — key, modifier snapshot at
/// push time, repeat flag. Modifiers and key events arrive
/// interleaved over the wire, so snapshotting at drain time would
/// mis-attribute mods on rapid chord input — `mods` is captured
/// when the event was pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Modifiers,
    /// `true` for OS-level key-repeat re-emissions; `false` for the
    /// initial press. Editors typically treat both the same; some
    /// commands (e.g. focus-cycle on Tab) only fire on `!repeat`.
    pub repeat: bool,
}

/// One entry in [`InputState::frame_keyboard_events`] — a press or
/// an IME-committed text chunk, in event-arrival order. Releases
/// (`KeyUp`) aren't surfaced: editors care about presses, and adding
/// a release variant without a consumer would invent state we don't
/// yet need.
///
/// [`InputState::frame_keyboard_events`]: crate::input::InputState
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardEvent {
    /// Logical key pressed.
    Down(KeyPress),
    /// Committed text from typing or an IME composition that just
    /// finalized. Distinct from `Down` because IME / dead-key
    /// composition produces text without a physical keypress, and
    /// because keys like `Enter` produce a `Down` but no text to
    /// insert.
    Text(TextChunk),
}

/// Translate a winit logical key into our [`Key`]. `Other` is the
/// catch-all so unrecognized keys still surface as a press without
/// dropping the event entirely.
pub(crate) fn key_from_winit(k: &winit::keyboard::Key) -> Key {
    use winit::keyboard::{Key as WK, NamedKey};
    match k {
        WK::Named(NamedKey::ArrowLeft) => Key::ArrowLeft,
        WK::Named(NamedKey::ArrowRight) => Key::ArrowRight,
        WK::Named(NamedKey::ArrowUp) => Key::ArrowUp,
        WK::Named(NamedKey::ArrowDown) => Key::ArrowDown,
        WK::Named(NamedKey::Backspace) => Key::Backspace,
        WK::Named(NamedKey::Delete) => Key::Delete,
        WK::Named(NamedKey::Home) => Key::Home,
        WK::Named(NamedKey::End) => Key::End,
        WK::Named(NamedKey::PageUp) => Key::PageUp,
        WK::Named(NamedKey::PageDown) => Key::PageDown,
        WK::Named(NamedKey::Enter) => Key::Enter,
        WK::Named(NamedKey::Tab) => Key::Tab,
        WK::Named(NamedKey::Escape) => Key::Escape,
        WK::Named(NamedKey::F1) => Key::F1,
        WK::Named(NamedKey::F2) => Key::F2,
        WK::Named(NamedKey::F3) => Key::F3,
        WK::Named(NamedKey::F4) => Key::F4,
        WK::Named(NamedKey::F5) => Key::F5,
        WK::Named(NamedKey::F6) => Key::F6,
        WK::Named(NamedKey::F7) => Key::F7,
        WK::Named(NamedKey::F8) => Key::F8,
        WK::Named(NamedKey::F9) => Key::F9,
        WK::Named(NamedKey::F10) => Key::F10,
        WK::Named(NamedKey::F11) => Key::F11,
        WK::Named(NamedKey::F12) => Key::F12,
        WK::Named(NamedKey::Space) => Key::Char(' '),
        WK::Character(s) => s.chars().next().map(Key::Char).unwrap_or(Key::Other),
        _ => Key::Other,
    }
}

pub(crate) fn modifiers_from_winit(m: &winit::keyboard::ModifiersState) -> Modifiers {
    // Normalize the platform's primary command key into `ctrl` here, at
    // the boundary, so nothing downstream branches on platform: Cmd
    // (super) on macOS, physical Ctrl elsewhere. On macOS the physical
    // Control key is kept separately in `mac_ctrl` for the rare binding
    // that wants it.
    let mac = matches!(PLATFORM, Platform::Mac);
    Modifiers {
        shift: m.shift_key(),
        ctrl: if mac { m.super_key() } else { m.control_key() },
        alt: m.alt_key(),
        mac_ctrl: mac && m.control_key(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_chunk_new_handles_cap_boundary() {
        // (label, input, expect_some, expect_empty).
        let cases: &[(&str, &str, bool, bool)] = &[
            ("multibyte_roundtrip", "héllo", true, false),
            ("at_capacity_15_bytes", "0123456789abcde", true, false),
            ("empty", "", true, true),
            ("over_capacity_16_bytes", "0123456789abcdef", false, false),
        ];
        for (label, s, expect_some, expect_empty) in cases {
            let c = TextChunk::new(s);
            assert_eq!(c.is_some(), *expect_some, "case {label}: some-ness");
            if let Some(c) = c {
                assert_eq!(c.as_str(), *s, "case {label}: roundtrip");
                assert_eq!(c.as_str().is_empty(), *expect_empty, "case {label}: empty");
            }
        }
    }

    #[test]
    fn modifiers_any_command_excludes_shift() {
        assert!(
            !Modifiers {
                shift: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
        assert!(
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
        assert!(
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
    }
}
