//! The text one key press produced, as the input queue carries it.

use std::fmt;
use std::str;
use tinyvec::ArrayVec;

/// One definition of [`KeyText::CAPACITY`]: a struct cannot name its own associated const in field types.
const CAPACITY: usize = 14;

/// The text a key press produced, riding beside its key.
///
/// **A key is not its text**: a press yields a chord to match and a string to type, so [`Shortcut`](crate::Shortcut) reads the key and [`TextEdit`](crate::TextEdit) the text.
///
/// **Inline and `Copy`**, so `InputEvent` stays `Copy`; longer text is an IME commit, split into several presses.
///
/// **Control characters never enter**: they are keys (Enter reports `"\r"`), filtered once, here.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyText {
    utf8: ArrayVec<[u8; CAPACITY]>,
}

impl KeyText {
    /// UTF-8 bytes one press may carry; sized for a 16-byte value (`ArrayVec` spends two on length).
    pub const CAPACITY: usize = CAPACITY;

    /// No text.
    pub const EMPTY: Self = Self {
        utf8: ArrayVec::from_array_empty([0; CAPACITY]),
    };

    /// `text` minus control characters, truncated between characters at [`Self::CAPACITY`].
    pub fn new(text: &str) -> Self {
        let mut out = Self::EMPTY;
        for c in text.chars() {
            if !out.push(c) {
                break;
            }
        }
        out
    }

    /// Append `c` unless it is a control character; `false` when it does not fit (where an IME commit splits).
    pub(crate) fn push(&mut self, c: char) -> bool {
        if c.is_control() {
            return true;
        }
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf).as_bytes();
        if self.utf8.len() + encoded.len() > CAPACITY {
            return false;
        }
        self.utf8.extend_from_slice(encoded);
        true
    }

    /// Text of one character.
    pub fn from_char(c: char) -> Self {
        let mut buf = [0u8; 4];
        Self::new(c.encode_utf8(&mut buf))
    }

    /// The text, or `""` for a press that produced none.
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.utf8).expect("whole characters, encoded on the way in")
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.utf8.is_empty()
    }
}

/// Prints the text, not the padding behind it.
impl fmt::Debug for KeyText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::key_text::KeyText;

    impl KeyText {
        pub(crate) fn of_key(key: Key) -> Self {
            match key {
                Key::Char(c) => Self::from_char(c),
                _ => Self::EMPTY,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::input::keyboard::key_text::KeyText;

    #[test]
    fn text_arrives_whole_and_prints_as_itself() {
        let two = KeyText::new("^e");
        assert_eq!(
            two.as_str(),
            "^e",
            "a dead-key fallback keeps both characters"
        );
        assert_eq!(format!("{two:?}"), "\"^e\"");
        assert_eq!(KeyText::from_char('é').as_str(), "é");
        assert!(KeyText::EMPTY.is_empty());
        assert_eq!(KeyText::EMPTY.as_str(), "");
        assert!(!two.is_empty());
    }

    #[test]
    fn control_characters_never_enter() {
        for control in ["\r", "\n", "\t", "\u{1}", "\u{7f}"] {
            assert!(
                KeyText::new(control).is_empty(),
                "{control:?} is a key, not text",
            );
        }
        assert_eq!(
            KeyText::new("a\rb").as_str(),
            "ab",
            "and one among text is dropped where it sits",
        );
    }

    #[test]
    fn a_long_commit_truncates_between_characters() {
        // Four-byte characters: three fit in 15 bytes, no half of a fourth.
        let wide = "𐍈𐍈𐍈𐍈";
        let held = KeyText::new(wide);
        assert_eq!(held.as_str(), "𐍈𐍈𐍈", "three whole characters, not 14 bytes");
        assert_eq!(held.as_str().len(), 12);
        let narrow = "abcdefghijklmnop";
        assert_eq!(KeyText::new(narrow).as_str(), "abcdefghijklmn");
    }
}
