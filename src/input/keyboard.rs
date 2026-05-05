//! Keyboard event vocabulary. The shape was sized for `TextEdit`'s
//! step-1 needs: a small `Key` enum covering navigation/editing keys
//! plus printable characters, a `Modifiers` struct, and an inline
//! `TextChunk` so [`crate::input::InputEvent`] stays `Copy`.
//!
//! Translation lives in [`crate::input::InputEvent::from_winit`].
//! Today nothing consumes these events — they fall through
//! [`crate::input::InputState::on_input`] silently. Step 2 (frame
//! queues) and step 3 (focus) wire the consumers.

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
    Space,
    /// Printable character, post-layout (post-shift).
    Char(char),
    /// Any key not covered by the variants above. Carried so dispatch
    /// can ignore it cleanly without translation losing the keypress.
    Other,
}

/// Modifier-key state. Sent as a standalone [`InputEvent::ModifiersChanged`]
/// whenever the held set changes; widgets read the latest snapshot from
/// [`InputState`] (wiring in step 2).
///
/// `meta` is Cmd on macOS, Super on Linux, Win on Windows — single
/// "platform modifier" slot, same convention as egui.
///
/// [`InputEvent::ModifiersChanged`]: crate::input::InputEvent::ModifiersChanged
/// [`InputState`]: crate::input::InputState
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// True if any of ctrl/alt/meta is held — the canonical "this is a
    /// shortcut, not text" predicate. Shift alone doesn't count
    /// (shift+letter is just the capitalized letter).
    pub const fn any_command(self) -> bool {
        self.ctrl || self.alt || self.meta
    }
}

/// Inline UTF-8 byte buffer carried by [`InputEvent::Text`]. Sized for
/// the common case (a single grapheme cluster ≤ 15 bytes); IME commits
/// longer than that split across multiple events at the translation
/// boundary. Inline storage keeps `InputEvent: Copy`.
///
/// [`InputEvent::Text`]: crate::input::InputEvent::Text
const TEXT_CHUNK_CAP: usize = 15;

#[derive(Clone, Copy)]
pub struct TextChunk {
    bytes: [u8; TEXT_CHUNK_CAP],
    len: u8,
}

impl TextChunk {
    pub const EMPTY: Self = Self {
        bytes: [0; TEXT_CHUNK_CAP],
        len: 0,
    };

    /// Build a chunk from `s`. Returns `None` if `s` exceeds the inline
    /// capacity. Callers translating from winit should split at
    /// grapheme boundaries before calling — never split mid-codepoint.
    pub fn new(s: &str) -> Option<Self> {
        if s.len() > TEXT_CHUNK_CAP {
            return None;
        }
        let mut bytes = [0u8; TEXT_CHUNK_CAP];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Some(Self {
            bytes,
            len: s.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: `from_str` only stores valid UTF-8 from a `&str`,
        // and `len` always reflects the byte count written.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len as usize]) }
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
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

/// One captured `KeyDown` event sitting on
/// [`InputState::frame_keys`] waiting for a focused widget to drain
/// it. Modifiers are captured at the moment the event was pushed —
/// modifier state and key events arrive interleaved over the wire, so
/// snapshotting at drain time would mis-attribute mods on rapid
/// chord input.
///
/// Releases (`KeyUp`) aren't queued: editors care about presses, and
/// adding a release queue without a consumer would invent state we
/// don't yet need.
///
/// [`InputState::frame_keys`]: crate::input::InputState
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Modifiers,
    /// `true` for OS-level key-repeat re-emissions; `false` for the
    /// initial press. Editors typically treat both the same; some
    /// commands (e.g. focus-cycle on Tab) only fire on `!repeat`.
    pub repeat: bool,
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
        WK::Named(NamedKey::Space) => Key::Space,
        WK::Character(s) => s.chars().next().map(Key::Char).unwrap_or(Key::Other),
        _ => Key::Other,
    }
}

pub(crate) fn modifiers_from_winit(m: &winit::keyboard::ModifiersState) -> Modifiers {
    Modifiers {
        shift: m.shift_key(),
        ctrl: m.control_key(),
        alt: m.alt_key(),
        meta: m.super_key(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_chunk_roundtrip() {
        let c = TextChunk::new("héllo").unwrap();
        assert_eq!(c.as_str(), "héllo");
        assert!(!c.is_empty());
    }

    #[test]
    fn text_chunk_too_long_returns_none() {
        // 16 bytes of ASCII = one over capacity.
        assert!(TextChunk::new("0123456789abcdef").is_none());
    }

    #[test]
    fn text_chunk_at_capacity() {
        let s = "0123456789abcde"; // exactly 15 bytes
        let c = TextChunk::new(s).unwrap();
        assert_eq!(c.as_str(), s);
    }

    #[test]
    fn text_chunk_empty() {
        let c = TextChunk::new("").unwrap();
        assert!(c.is_empty());
        assert_eq!(c.as_str(), "");
    }

    #[test]
    fn text_chunk_empty_constant_matches_constructed_empty() {
        assert!(TextChunk::EMPTY.is_empty());
        assert_eq!(TextChunk::EMPTY.as_str(), "");
        assert_eq!(TextChunk::EMPTY, TextChunk::new("").unwrap());
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
                meta: true,
                ..Modifiers::NONE
            }
            .any_command()
        );
    }
}
