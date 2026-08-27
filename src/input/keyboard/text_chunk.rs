//! Typed or IME-committed text, inline in the event rather than behind an
//! allocation, sized for the runs a keystroke actually produces.

/// Inline UTF-8 byte buffer carried by [`InputEvent::Text`]. Sized for
/// the common case (a single grapheme cluster ≤ 15 bytes); longer IME
/// commits split across multiple events at the translation boundary.
/// Inline storage keeps `InputEvent: Copy`.
///
/// [`InputEvent::Text`]: crate::input::input_event::InputEvent::Text
#[derive(Clone, Copy)]
pub struct TextChunk {
    bytes: [u8; Self::INLINE_CAP],
    len: u8,
}

impl TextChunk {
    /// Longest UTF-8 byte sequence one chunk can hold. Text past this is
    /// split across several [`InputEvent::Text`](crate::InputEvent::Text)
    /// events at char boundaries.
    pub const INLINE_CAP: usize = 15;

    /// Build a chunk from `s`. Returns `None` if `s` exceeds the inline
    /// capacity. Callers with longer text split at char boundaries
    /// first (see `emit_text_chunks`) — never mid-codepoint. Grapheme
    /// clusters may split across chunks; consumers re-assemble on
    /// append.
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

    /// The chunk's text. Always valid UTF-8, but may end mid-grapheme
    /// when a long commit was split — consumers re-assemble on append.
    pub fn as_str(&self) -> &str {
        // SAFETY: `new` only stores valid UTF-8 from a `&str`,
        // and `len` always reflects the byte count written.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len as usize]) }
    }

    /// Split `s` into chunks at char boundaries, each within
    /// [`Self::INLINE_CAP`] — the shape an IME commit arrives in.
    /// An empty `s` yields nothing.
    pub fn split(s: &str) -> impl Iterator<Item = Self> + '_ {
        let mut rest = s;
        std::iter::from_fn(move || {
            if rest.is_empty() {
                return None;
            }
            let mut end = rest.len().min(Self::INLINE_CAP);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let (head, tail) = rest.split_at(end);
            rest = tail;
            Some(Self::new(head).expect("chunk fits by construction"))
        })
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

#[cfg(test)]
mod tests {
    use crate::input::keyboard::text_chunk::TextChunk;

    #[test]
    fn new_handles_cap_boundary() {
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
}
