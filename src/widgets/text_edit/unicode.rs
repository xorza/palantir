//! Unicode-aware helpers over the host's buffer: grapheme and word
//! boundaries for caret motion, the word range a double-click selects,
//! and the line-break scrub a single-line field applies to inbound text.
//!
//! Free functions rather than methods: they answer about a `&str` the
//! widget does not own, and none of them needs `EditState`.

use std::borrow::Cow;

/// Strip line-break chars from an inbound string so the single-line
/// TextEdit's buffer never contains `\n` / `\r`. Hit by both the
/// paste path and the IME-text-commit path — host events and OS
/// clipboards routinely carry `\r\n` / `\n` from multi-line sources
/// that this widget can't render or hit-test correctly. Spaces are a
/// safer substitute than outright deletion (preserves intent for
/// "First Name\nLast Name" → "First Name Last Name"). Borrowed
/// pass-through on the common break-free case — no per-keystroke
/// allocation.
pub(super) fn sanitize_single_line(s: &str) -> Cow<'_, str> {
    if memchr::memchr2(b'\n', b'\r', s.as_bytes()).is_none() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_was_break = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            // Collapse `\r\n` and runs of breaks into a single space.
            if !prev_was_break {
                out.push(' ');
            }
            prev_was_break = true;
        } else {
            out.push(ch);
            prev_was_break = false;
        }
    }
    Cow::Owned(out)
}

/// Next grapheme-cluster boundary strictly after `offset` (clamped to
/// `text.len()`). Walks extended grapheme clusters via
/// [`unicode_segmentation::GraphemeCursor`] so multi-codepoint clusters
/// (combining marks, ZWJ-joined family emoji) advance as one unit.
pub(super) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// Previous grapheme-cluster boundary strictly before `offset` (clamped
/// to zero).
pub(super) fn prev_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

/// Coarse char classification used by word-nav and double-click word
/// selection. Underscore is bound to `Word` so identifiers in code-like
/// text don't fragment. Codepoint-granular — fine for Latin / digit /
/// mixed text; a Unicode word-break iterator would do better on CJK
/// and friends but isn't wired yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharKind {
    Whitespace,
    Word,
    Other,
}

fn char_kind(c: char) -> CharKind {
    if c.is_whitespace() {
        CharKind::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else {
        CharKind::Other
    }
}

/// Forward word boundary: skip whitespace, then skip the run of
/// same-`CharKind` chars. Returns `text.len()` if `from` is already at
/// the end. The result is the byte index *just past* the end of the
/// consumed word run — same convention as `Ctrl+Right` in most editors.
pub(super) fn next_word_boundary(text: &str, from: usize) -> usize {
    let mut chars = text[from..].char_indices();
    let mut pos;
    let target_kind = loop {
        let Some((i, c)) = chars.next() else {
            return text.len();
        };
        if char_kind(c) != CharKind::Whitespace {
            pos = from + i + c.len_utf8();
            break char_kind(c);
        }
    };
    for (i, c) in chars {
        if char_kind(c) == target_kind {
            pos = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    pos
}

/// Mirror of [`next_word_boundary`]. Walks backward from `from` over
/// whitespace and then over the run of same-`CharKind` chars; returns
/// the byte index of the first consumed char (start of that run).
pub(super) fn prev_word_boundary(text: &str, from: usize) -> usize {
    let mut rev = text[..from].char_indices().rev();
    let mut pos;
    let target_kind = loop {
        let Some((i, c)) = rev.next() else {
            return 0;
        };
        if char_kind(c) != CharKind::Whitespace {
            pos = i;
            break char_kind(c);
        }
    };
    for (i, c) in rev {
        if char_kind(c) == target_kind {
            pos = i;
        } else {
            break;
        }
    }
    pos
}

/// Word range surrounding `byte`. Returns the smallest `[start, end)`
/// such that every char in it shares one `CharKind` and `byte` lies on
/// or just past a boundary inside the run. Whitespace runs collapse to
/// `byte..byte` so a double-click on a space doesn't select the gap.
/// Used by double-click word selection.
pub(super) fn word_range_at(text: &str, byte: usize) -> std::ops::Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let byte = byte.min(text.len());
    // Pick the char that "anchors" this position: the one at `byte`
    // (forward) if it's word/other, otherwise the char before `byte`
    // (so a trailing-edge caret on the last char of a word still
    // selects that word).
    let forward_char = text[byte..].chars().next();
    let backward_char = text[..byte].chars().next_back();
    let anchor_kind = match (forward_char.map(char_kind), backward_char.map(char_kind)) {
        (Some(CharKind::Whitespace) | None, Some(k)) if k != CharKind::Whitespace => k,
        (Some(k), _) if k != CharKind::Whitespace => k,
        _ => return byte..byte,
    };
    // Walk left while same kind. When the anchor is the *backward*
    // char (forward char is whitespace / EOT), step `start` back over
    // it first — that char ends at `byte`, so it starts at
    // `byte - c.len_utf8()`. Otherwise the forward char is the anchor
    // and `start` already points at its start.
    let mut start = byte;
    if !forward_char.is_some_and(|c| char_kind(c) == anchor_kind)
        && let Some(c) = backward_char
    {
        start = byte - c.len_utf8();
    }
    for (i, c) in text[..start].char_indices().rev() {
        if char_kind(c) == anchor_kind {
            start = i;
        } else {
            break;
        }
    }
    // Walk right while same kind.
    let mut end = byte;
    for (i, c) in text[end..].char_indices() {
        if char_kind(c) == anchor_kind {
            end = byte + i + c.len_utf8();
        } else {
            break;
        }
    }
    start..end
}
