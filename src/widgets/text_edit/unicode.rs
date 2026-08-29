//! Unicode-aware helpers over the host's buffer: grapheme and word
//! boundaries for caret motion, the word range a double-click selects,
//! and the line-break scrub a single-line field applies to inbound text.
//!
//! Free functions rather than methods: they answer about a `&str` the
//! widget does not own, and none of them needs `EditState`.

use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

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
///
/// **Both outcomes below are unreachable, and both are stated rather than
/// defaulted.** The cursor is handed the whole string as its one chunk at
/// offset zero, so it can never ask for more context, and the early return
/// above means there is text left for a boundary to sit in. A silent
/// `text.len()` for either would answer a caret question with the end of
/// the buffer — a wrong caret, not a crash, which is the failure a text
/// field cannot be debugged through.
pub(super) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor
        .next_boundary(text, 0)
        .expect(CHUNK_IS_WHOLE)
        .expect("an offset inside the text is always followed by a boundary")
}

/// Previous grapheme-cluster boundary strictly before `offset` (clamped
/// to zero). Unreachable outcomes are stated for the reason
/// [`next_grapheme_boundary`] gives.
pub(super) fn prev_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor
        .prev_boundary(text, 0)
        .expect(CHUNK_IS_WHOLE)
        .expect("a nonzero offset is always preceded by a boundary")
}

/// Why neither cursor walk can report incomplete context.
const CHUNK_IS_WHOLE: &str =
    "the cursor holds the whole string as one chunk, so it cannot need more context";

/// What one UAX #29 word-bound segment is, as far as caret motion and
/// double-click selection care.
///
/// Classified per *segment* rather than per codepoint, which is what
/// gives `3.14`, `don't` and `foo_bar` one word each and splits a CJK
/// run at its real word boundaries — none of which a
/// `char::is_alphanumeric` test can see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentKind {
    Whitespace,
    Word,
    Other,
}

impl SegmentKind {
    /// A word-bound segment is homogeneous, so its first character
    /// settles whitespace. A word segment may still carry a `'` or a `.`
    /// inside it, so the word test asks whether *any* character is one.
    fn of(seg: &str) -> Self {
        match seg.chars().next() {
            Some(c) if c.is_whitespace() => Self::Whitespace,
            Some(_) if seg.chars().any(|c| c.is_alphanumeric() || c == '_') => Self::Word,
            _ => Self::Other,
        }
    }
}

/// Walk `segments` past any whitespace, then across the run of one kind,
/// and answer `edge` of the last segment consumed. `None` when there was
/// nothing but whitespace left.
///
/// One body for both directions: [`next_word_boundary`] feeds forward
/// segments and takes each one's far end, [`prev_word_boundary`] feeds
/// them in reverse and takes each one's start.
///
/// A punctuation run crosses as one. UAX #29 gives each mark a segment of
/// its own, which is right for a word iterator and wrong for a caret:
/// `Ctrl+Right` over `-->` should cross the arrow, not one dash of it.
fn scan_run<'a>(
    segments: impl Iterator<Item = (usize, &'a str)>,
    edge: impl Fn(usize, &'a str) -> usize,
) -> Option<usize> {
    let mut segments =
        segments.skip_while(|(_, seg)| SegmentKind::of(seg) == SegmentKind::Whitespace);
    let (i, seg) = segments.next()?;
    let mut pos = edge(i, seg);
    if SegmentKind::of(seg) == SegmentKind::Other {
        for (i, seg) in segments {
            if SegmentKind::of(seg) != SegmentKind::Other {
                break;
            }
            pos = edge(i, seg);
        }
    }
    Some(pos)
}

/// Forward word boundary: skip whitespace, then cross one run. Returns
/// `text.len()` if only whitespace remains. The result is the byte index
/// *just past* the run — the `Ctrl+Right` convention.
pub(super) fn next_word_boundary(text: &str, from: usize) -> usize {
    scan_run(text[from..].split_word_bound_indices(), |i, seg| {
        from + i + seg.len()
    })
    .unwrap_or(text.len())
}

/// Mirror of [`next_word_boundary`]: the byte index the run starts at.
pub(super) fn prev_word_boundary(text: &str, from: usize) -> usize {
    scan_run(text[..from].split_word_bound_indices().rev(), |i, _| i).unwrap_or(0)
}

/// The run surrounding `byte`, for double-click word selection.
///
/// Whitespace collapses to `byte..byte`, so a double-click on a gap
/// selects nothing. A caret sitting on a run's trailing edge selects the
/// run behind it, which is what makes a double-click at the end of the
/// last word still take that word.
pub(super) fn word_range_at(text: &str, byte: usize) -> std::ops::Range<usize> {
    let byte = byte.min(text.len());
    let mut prev: Option<std::ops::Range<usize>> = None;
    let mut segments = text.split_word_bound_indices().peekable();
    while let Some((start, seg)) = segments.next() {
        let kind = SegmentKind::of(seg);
        let mut end = start + seg.len();
        // Punctuation selects as one run — see `scan_run`.
        if kind == SegmentKind::Other {
            while segments
                .peek()
                .is_some_and(|(_, next)| SegmentKind::of(next) == SegmentKind::Other)
            {
                let (i, next) = segments.next().expect("peeked");
                end = i + next.len();
            }
        }
        if byte < end {
            if kind != SegmentKind::Whitespace {
                return start..end;
            }
            break;
        }
        prev = (kind != SegmentKind::Whitespace).then_some(start..end);
    }
    match prev {
        Some(range) if range.end == byte => range,
        _ => byte..byte,
    }
}
