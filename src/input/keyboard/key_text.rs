//! The text one key press produced, as the input queue carries it.

use std::fmt;
use tinyvec::ArrayVec;

/// [`KeyText::CAP`]'s one definition, because a struct cannot name its
/// own associated const in its field types.
const CAP: usize = 14;

/// The text a key press produced — what the platform resolved the
/// layout, the dead keys and the modifiers *into*, riding beside the key
/// that produced it.
///
/// **A key is not its text, and neither derives from the other.** One
/// press yields a chord to match and a string to type: a dead key
/// composes two presses into `é`, a Windows dead-key fallback resolves
/// to the two characters `^e`, and a layout can put any character on any
/// key. Matching a command against the text, or typing the key's own
/// name, each break a case the other handles. So a press carries both,
/// and each consumer reads the one it means —
/// [`Shortcut`](crate::Shortcut) the key,
/// [`TextEdit`](crate::TextEdit) the text.
///
/// **Inline and `Copy`**, so [`InputEvent`](crate::InputEvent) stays
/// `Copy` and the per-frame queue stays one flat vector. A key press
/// produces one grapheme or two, which [`Self::CAP`] holds several times
/// over; text longer than that is an IME commit, and this vocabulary
/// does not carry one.
///
/// **Control characters never enter.** They are keys rather than text —
/// Enter reports `"\r"` on Windows, Tab `"\t"`, Ctrl+A `"\u{1}"` — and a
/// field that typed them would write a carriage return where the user
/// pressed Enter. Every consumer would otherwise owe the same filter,
/// so it happens once, here, on the way in.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyText {
    utf8: ArrayVec<[u8; CAP]>,
}

impl KeyText {
    /// How many UTF-8 bytes one press may carry: three of the widest
    /// characters a keyboard produces, or fourteen of the narrowest.
    /// Sized so the whole value is 16 bytes, since `ArrayVec` spends two
    /// on its own length.
    pub const CAP: usize = CAP;

    /// A press that produced no text — a named key, or a dead key still
    /// waiting for the one that completes it.
    pub const EMPTY: Self = Self {
        utf8: ArrayVec::from_array_empty([0; CAP]),
    };

    /// `text` with its control characters dropped, truncated between
    /// characters once [`Self::CAP`] is full.
    pub fn new(text: &str) -> Self {
        let mut out = Self::EMPTY;
        for c in text.chars().filter(|c| !c.is_control()) {
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf).as_bytes();
            if out.utf8.len() + encoded.len() > CAP {
                break;
            }
            out.utf8.extend_from_slice(encoded);
        }
        out
    }

    /// One character as its own text — the press a keyboard mostly
    /// makes, spelled without a string to hold it.
    pub fn from_char(c: char) -> Self {
        let mut buf = [0u8; 4];
        Self::new(c.encode_utf8(&mut buf))
    }

    /// The text, or `""` for a press that produced none.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.utf8).expect("whole characters, encoded on the way in")
    }

    /// Whether the press produced no text to type.
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

    /// A control character is a key, not text: Enter reports `"\r"` and
    /// Tab `"\t"`, and a field that typed them would write the byte the
    /// key stands for.
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

    /// The cap truncates on a character boundary, so what survives is
    /// still text.
    #[test]
    fn a_long_commit_truncates_between_characters() {
        // Four-byte characters: three fit in 15 bytes, the fourth does
        // not, and no half of it may land.
        let wide = "𐍈𐍈𐍈𐍈";
        let held = KeyText::new(wide);
        assert_eq!(held.as_str(), "𐍈𐍈𐍈", "three whole characters, not 14 bytes");
        assert_eq!(held.as_str().len(), 12);
        // One byte per character fills the cap exactly.
        let narrow = "abcdefghijklmnop";
        assert_eq!(KeyText::new(narrow).as_str(), "abcdefghijklmn");
    }
}
