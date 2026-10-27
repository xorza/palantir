//! Deterministic placeholder shaping for the mono fallback: every glyph is `font_size * 0.5` wide, so layout tests state widths as arithmetic. Reached only through [`TextShaper::test_mono`](crate::text::shaper::TextShaper), so the module is test-gated.

use crate::primitives::geometry::size::Size;
use crate::text::extent::TextExtent;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{self, LineFit, WrapFloor};
use std::iter;

/// Width of one `char` at `font_size`, whatever its UTF-8 length.
fn glyph_width(font_size: f32) -> f32 {
    font_size * 0.5
}

/// Caret-x along a single-line mono layout: one glyph width per `char` before `byte_offset`.
pub(super) fn single_line_caret_x(text: &str, byte_offset: usize, font_size: f32) -> f32 {
    let clamped = text.floor_char_boundary(byte_offset.min(text.len()));
    text[..clamped].chars().count() as f32 * glyph_width(font_size)
}

/// Inverse of [`single_line_caret_x`]: the char boundary whose prefix-x is closest to `target_x`; the earliest on a tie.
pub(super) fn nearest_byte(text: &str, target_x: f32, font_size: f32) -> usize {
    let glyph_w = glyph_width(font_size);
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (chars, (i, ch)) in (1usize..).zip(text.char_indices()) {
        let next = i + ch.len_utf8();
        let x = chars as f32 * glyph_w;
        let d = (x - target_x).abs();
        if d < best_dist {
            best_dist = d;
            best_off = next;
        }
    }
    best_off
}

/// The run's unbounded shape under the mono metric (twin of [`CosmicMeasure::root`](crate::text::cosmic::CosmicMeasure)); mints no shaped buffer, so the renderer drops these runs.
pub(super) fn root(request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
    let glyph_w = glyph_width(request.key.font_size());
    let lines = Lines::of(request.text, None);
    TextRoot {
        // Mono reads no outlines: a cell holds its glyph.
        extent: TextExtent::inked_within(Size::new(
            lines.widest as f32 * glyph_w,
            lines.count as f32 * request.key.line_height(),
        )),
        intrinsic_min: (floor == WrapFloor::Scan)
            .then(|| intrinsic_min_width(request.text, glyph_w)),
        single_line: lines.count == 1,
    }
}

/// The extent this run resolves to at its key's committed width; twin of [`CosmicMeasure::resolve`](crate::text::cosmic::CosmicMeasure), routed by [`LineFit`].
pub(super) fn resolve(request: TextShapeRequest<'_>) -> TextExtent {
    let key = request.key;
    let text = request.text;
    let glyph_w = glyph_width(key.font_size());
    let line_h = key.line_height();
    let max = key.max_width().expect("a bounded resolve commits a width");
    TextExtent::inked_within(match key.fit() {
        // A run is cut inside its first line, as the cosmic cut reads the first layout run, and
        // a single line that fits keeps all of it.
        LineFit::Clip | LineFit::Ellipsis | LineFit::EllipsisMiddle => {
            let first = hard_lines(text)
                .next()
                .map_or(0, |line| line.chars().count());
            Size::new((first as f32 * glyph_w).min(max), line_h)
        }
        LineFit::Wrap => {
            #[expect(
                clippy::cast_sign_loss,
                reason = "a committed width and a glyph width are not negative"
            )]
            let per_line = ((max / glyph_w).floor() as usize).max(1);
            let lines = Lines::of(text, Some(per_line));
            Size::new(lines.widest as f32 * glyph_w, lines.count as f32 * line_h)
        }
    })
}

/// A run's lines and the widest one, in glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lines {
    count: usize,
    widest: usize,
}

impl Lines {
    /// `text` laid out as cosmic lays a buffer: a line per hard break, and, at `per_line` glyphs
    /// a line, wrapped at [`wrap::break_offsets`], a segment wider than a line split by glyphs,
    /// as `Wrap::WordOrGlyph` does. A segment's trailing whitespace hangs past the line, so it
    /// neither counts toward a fit nor widens the line.
    fn of(text: &str, per_line: Option<usize>) -> Self {
        let mut lines = Self {
            count: 0,
            widest: 0,
        };
        for line in hard_lines(text) {
            let Some(per_line) = per_line else {
                lines.push(line.chars().count());
                continue;
            };
            // Glyphs on the open line, and how many of them are not hanging whitespace.
            let mut used = 0;
            let mut shown = 0;
            let mut start = 0;
            let ends = wrap::break_offsets(line).chain(iter::once(line.len()));
            for end in ends {
                let segment = &line[start..end];
                start = end;
                let mut body = segment.trim_end().chars().count();
                let trailing = segment.chars().count() - body;
                if used > 0 && used + body > per_line {
                    lines.push(shown);
                    used = 0;
                }
                // A segment wider than a line fills whole lines by glyphs; its rest stays open.
                while used + body > per_line {
                    lines.push(per_line);
                    body -= per_line;
                }
                shown = used + body;
                used = shown + trailing;
            }
            lines.push(shown);
        }
        lines
    }

    fn push(&mut self, width: usize) {
        self.count += 1;
        self.widest = self.widest.max(width);
    }
}

/// The hard lines of `text`, each without its ending: split at `\n`, `\r`, `\r\n` and `\n\r`,
/// with the line after a final break kept though empty, as cosmic's `LineIter` and buffer split.
fn hard_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    iter::from_fn(move || {
        let line = rest?;
        let Some(at) = line.find(['\r', '\n']) else {
            rest = None;
            return Some(line);
        };
        let after = &line[at..];
        let ending = if after.starts_with("\r\n") || after.starts_with("\n\r") {
            2
        } else {
            1
        };
        rest = Some(&line[at + ending..]);
        Some(&line[..at])
    })
}

/// Widest unbreakable segment of `text` under a uniform `glyph_w`; twin of `cosmic::geometry::SegmentScratch::intrinsic_min_width`. Segments come from [`wrap::break_offsets`] with trailing whitespace dropped (the break sits after a space). Ceil'd like its twin: `WrapWithOverflow` floors a committed width at this.
fn intrinsic_min_width(text: &str, glyph_w: f32) -> f32 {
    let mut widest = 0usize;
    let mut start = 0usize;
    for next in wrap::break_offsets(text) {
        widest = widest.max(text[start..next].trim_end().chars().count());
        start = next;
    }
    widest = widest.max(text[start..].trim_end().chars().count());
    (widest as f32 * glyph_w).ceil()
}

#[cfg(test)]
mod tests {
    use crate::text::mono::{Lines, hard_lines};

    /// Every ending cosmic splits at, and the empty line after a final one.
    #[test]
    fn hard_lines_split_as_cosmic_does() {
        let cases: [(&str, &[&str]); 5] = [
            ("a\nb", &["a", "b"]),
            ("a\r\nb\n", &["a", "b", ""]),
            ("x\r", &["x", ""]),
            ("one\n\rtwo", &["one", "two"]),
            ("\n", &["", ""]),
        ];
        for (text, want) in cases {
            assert_eq!(hard_lines(text).collect::<Vec<_>>(), want, "{text:?}");
        }
    }

    /// Unbounded, a line per hard break, trailing spaces counted. At five glyphs a line:
    /// `"hello "` fits and its space hangs, so `"world"` opens line two; `"aa "` and `"bb"` fill
    /// five, `"cc"` opens the next; twelve unbroken glyphs fill two lines and leave two; ten
    /// fill exactly two. At four, `"ab cd"` breaks after `"ab"`, and the hard break's
    /// `"efghij"` fills one line and leaves two.
    #[test]
    fn lines_break_hard_then_wrap_at_break_offsets() {
        let lines = |count, widest| Lines { count, widest };
        assert_eq!(Lines::of("ab\nc", None), lines(2, 2));
        assert_eq!(Lines::of("a  ", None), lines(1, 3));
        let cases = [
            ("hello world", 5, lines(2, 5)),
            ("aa bb cc", 5, lines(2, 5)),
            ("abcdefghijkl", 5, lines(3, 5)),
            ("abcdefghij", 5, lines(2, 5)),
            ("ab cd\nefghij", 4, lines(4, 4)),
            ("ab", 5, lines(1, 2)),
        ];
        for (text, per_line, want) in cases {
            assert_eq!(
                Lines::of(text, Some(per_line)),
                want,
                "{text:?} at {per_line}"
            );
        }
    }
}
