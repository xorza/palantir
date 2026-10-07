//! Unicode helpers for the edit buffer: grapheme and word boundaries, the double-click word range, and the single-line scrub.

use std::borrow::Cow;
use std::ops;
use unicode_segmentation::UnicodeSegmentation;

/// Replaces line breaks with spaces so a single-line field never holds `\n` or `\r`; borrows when break-free.
pub(super) fn sanitize_single_line(s: &str) -> Cow<'_, str> {
    if !s.contains(['\n', '\r']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_was_break = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
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
/// `text.len()`). Both `expect`s are unreachable: the cursor holds the whole
/// string as one chunk.
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

const CHUNK_IS_WHOLE: &str =
    "the cursor holds the whole string as one chunk, so it cannot need more context";

/// What one UAX #29 word-bound segment is for caret motion; classified per segment, so `3.14` is one word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentKind {
    Whitespace,
    Word,
    Other,
}

impl SegmentKind {
    /// A segment is homogeneous, so its first character settles whitespace; a word may carry `'` or `.`.
    fn of(seg: &str) -> Self {
        match seg.chars().next() {
            Some(c) if c.is_whitespace() => Self::Whitespace,
            Some(_) if seg.chars().any(|c| c.is_alphanumeric() || c == '_') => Self::Word,
            _ => Self::Other,
        }
    }
}

/// Walks `segments` past whitespace, then across one run, and answers `edge`
/// of the last segment consumed (`None` if only whitespace remained). A
/// punctuation run crosses as one, so `Ctrl+Right` over `-->` crosses the arrow.
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

/// Forward word boundary: skips whitespace, then one run; `text.len()` if only whitespace remains.
pub(super) fn next_word_boundary(text: &str, from: usize) -> usize {
    scan_run(text[from..].split_word_bound_indices(), |i, seg| {
        from + i + seg.len()
    })
    .unwrap_or(text.len())
}

pub(super) fn prev_word_boundary(text: &str, from: usize) -> usize {
    scan_run(text[..from].split_word_bound_indices().rev(), |i, _| i).unwrap_or(0)
}

/// The run surrounding `byte` for double-click; whitespace collapses to `byte..byte`, a trailing edge selects the run behind.
pub(super) fn word_range_at(text: &str, byte: usize) -> ops::Range<usize> {
    let byte = byte.min(text.len());
    let mut prev: Option<ops::Range<usize>> = None;
    let mut segments = text.split_word_bound_indices().peekable();
    while let Some((start, seg)) = segments.next() {
        let kind = SegmentKind::of(seg);
        let mut end = start + seg.len();
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
