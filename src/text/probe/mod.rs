//! The public text-geometry surface: read-only caret placement, hit-testing and
//! selection rects over one shaped layout, reached through
//! [`Ui::probe_text`](crate::Ui::probe_text). Nothing shaped escapes `src/text/`;
//! the cosmic-text buffer stays private to this file.

use crate::common::hash;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::HAlign;
use crate::text::cosmic::shaped_buffer_cache::ShapedRun;
use crate::text::key::TextShapeKey;
use crate::text::shaper::ShaperInner;
use std::cell::RefMut;
use std::num::NonZeroU64;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Geometry queries over one shaped run, minted by
/// [`Ui::probe_text`](crate::Ui::probe_text).
///
/// **A live probe holds the shaper's exclusive borrow**, so it borrows the `Ui`
/// mutably: overlapping probes are E0499 at compile time, not a `RefCell` panic.
/// Sequential probes are fine.
#[derive(Debug)]
pub struct TextProbe<'a> {
    size: Size,
    text: &'a str,
    /// Key of the buffer this probe answers against; carried even where nothing was
    /// shaped, since the metrics live on it. `None` only for a face the shaper can't
    /// be asked for, which gives a zero-height band ([`Self::line_height`]).
    key: Option<TextShapeKey>,
    /// The run's authored horizontal alignment. Not read off `key`: an unbounded key
    /// stores `Auto` for every alignment, which would put a right-aligned glyphless
    /// line's caret at the block's left edge.
    halign: HAlign,
    inner: RefMut<'a, ShaperInner>,
}

impl<'a> TextProbe<'a> {
    pub(super) const fn new(
        size: Size,
        text: &'a str,
        key: Option<TextShapeKey>,
        halign: HAlign,
        inner: RefMut<'a, ShaperInner>,
    ) -> Self {
        Self {
            size,
            text,
            key,
            halign,
            inner,
        }
    }

    /// Extent of the shaped run; `Size::ZERO` for empty text.
    pub const fn size(&self) -> Size {
        self.size
    }

    /// 64-bit hash of the run's text as the shaping cache keys it, for "same string as
    /// last frame?" without a copy. `None` where the face names no size the shaper can
    /// be asked for.
    pub fn text_hash(&self) -> Option<NonZeroU64> {
        Some(self.key?.text_hash)
    }

    /// What [`Self::text_hash`] would answer for `text`, without shaping. A caller
    /// tracking a buffer's identity mixes both sources, so they must agree exactly; a
    /// mismatch reads as "buffer replaced" (`TextEdit` wipes its undo stack).
    ///
    /// ```
    /// # use palantir::widget::TextProbe;
    /// assert_eq!(TextProbe::hash_of("hello"), TextProbe::hash_of("hello"));
    /// assert_ne!(TextProbe::hash_of("hello"), TextProbe::hash_of("world"));
    /// ```
    pub fn hash_of(text: &str) -> NonZeroU64 {
        TextShapeKey::content_hash(hash::hash_str(text))
    }

    fn line_height(&self) -> f32 {
        self.key.map_or(0.0, TextShapeKey::line_height)
    }

    /// Shaped run behind this layout; `None` on the mono metric, for empty text, and
    /// for a face the shaper can't be asked for. Mono is refused rather than missed:
    /// its key is real, so a buffer another caller shaped under it would give cosmic
    /// geometry against a mono extent. Every query reports in block-local coordinates,
    /// so each subtracts [`ShapedRun::left`] from the buffer's x, or adds it back.
    fn shaped(&self) -> Option<ShapedRun<'_>> {
        if self.inner.is_mono() {
            return None;
        }
        self.inner.cosmic().shaped_run(self.key?)
    }

    /// True where this run had nothing to shape (no bytes, or a face with no usable
    /// size).
    const fn shapes_nothing(&self) -> bool {
        self.text.is_empty() || self.key.is_none()
    }

    /// Caret-x for a layout with no shaped buffer. Production reaches this for runs
    /// that shape nothing: the block is zero-width and the only position is its
    /// origin. The mono metric also lands here, answered by `mono` (named by full
    /// path; its module is gated out of production). Anything else is a wiring bug.
    /// The gate asks which metric measured the run, not whether the text is empty, so
    /// a cosmic run that lost its buffer trips the assertion instead of taking a mono
    /// estimate.
    fn unshaped_caret_x(&self, byte_offset: usize) -> f32 {
        #[cfg(any(test, feature = "internals"))]
        #[expect(
            clippy::absolute_paths,
            reason = "a gated statement names the path inline instead of a cfg'd import"
        )]
        if let Some(key) = self.key.filter(|_| self.inner.is_mono()) {
            return crate::text::mono::single_line_caret_x(self.text, byte_offset, key.font_size());
        }
        assert!(
            self.shapes_nothing(),
            "a shapeable run with no shaped buffer requires the mono metric \
             (caret at byte {byte_offset})",
        );
        0.0
    }

    /// [`Self::unshaped_caret_x`]'s inverse; production answers 0.
    fn unshaped_byte_at(&self, target_x: f32) -> usize {
        #[cfg(any(test, feature = "internals"))]
        #[expect(
            clippy::absolute_paths,
            reason = "a gated statement names the path inline instead of a cfg'd import"
        )]
        if let Some(key) = self.key.filter(|_| self.inner.is_mono()) {
            return crate::text::mono::nearest_byte(self.text, target_x, key.font_size());
        }
        assert!(
            self.shapes_nothing(),
            "a shapeable run with no shaped buffer requires the mono metric \
             (hit-test at x {target_x})",
        );
        0
    }

    /// Where the caret sits at `byte_offset`. Multi-line aware; the mono and
    /// empty-text path is 1D (`y_top = 0`, flat per-byte `x`). Horizontal placement
    /// uses `LayoutRun::cursor_position`, the geometry `Buffer::hit` inverts, so
    /// hit-test then caret round-trips; it handles RTL glyphs and offsets inside
    /// ligature or Indic clusters. `byte_offset` is clamped to the run.
    pub fn caret_at(&self, byte_offset: usize) -> Caret {
        let line_height = self.line_height();
        let halign = self.halign;
        let Some(ShapedRun { buffer, left }) = self.shaped() else {
            // No shaped buffer: empty text or the mono metric.
            return Caret {
                x: self.unshaped_caret_x(byte_offset),
                y_top: 0.0,
                line_height,
            };
        };
        let target = LineMap::new(buffer, self.text).cursor(byte_offset);

        let mut last_in_line: Option<Caret> = None;
        for run in buffer.layout_runs() {
            if run.line_i != target.line {
                continue;
            }
            let at = |x: f32| Caret {
                x: x - left,
                y_top: run.line_top,
                line_height: run.line_height,
            };
            // A glyphless line has no glyph to hang the caret on and cosmic reports x = 0;
            // place it where per-line align puts the first typed glyph (already block-local).
            if run.glyphs.is_empty() {
                return Caret {
                    x: empty_line_x(self.size.w, halign),
                    y_top: run.line_top,
                    line_height: run.line_height,
                };
            }
            match run.cursor_position(&target) {
                Some(x) => return at(x),
                // Soft wrap splits a logical line across runs; a miss means a later run.
                None => {
                    last_in_line = Some(at(run.glyphs.last().map_or(0.0, |g| g.x + g.w)));
                }
            }
        }
        last_in_line.unwrap_or(Caret {
            x: 0.0,
            y_top: 0.0,
            line_height,
        })
    }

    /// The byte offset a point lands on, in run-local coordinates (click-to-caret),
    /// clamped to the run. Multi-line via `Buffer::hit`; the mono and empty-text path
    /// scans one dimension by `x ÷ 0.5·font_size` and ignores `y`.
    pub fn byte_at(&self, x: f32, y: f32) -> usize {
        match self.shaped() {
            // `x` is block-local; add it back to buffer space (inverse of [`Self::caret_at`]).
            Some(ShapedRun { buffer, left }) => {
                buffer.hit(x + left, y).map_or(self.text.len(), |cursor| {
                    LineMap::new(buffer, self.text).byte(cursor)
                })
            }
            None => self.unshaped_byte_at(x),
        }
    }

    /// Every rect covering `range`, one per visual line, in run-local coordinates;
    /// `range` is clamped. A callback so nothing allocates per frame
    /// (`long_multiline_selection_alloc_free` pins it).
    pub fn selection_rects(&self, range: Range<usize>, mut out: impl FnMut(Rect)) {
        if range.is_empty() {
            return;
        }
        let Some(ShapedRun { buffer, left }) = self.shaped() else {
            // No shaped buffer: mono lays the band out 1D, empty text collapses it.
            let x0 = self.unshaped_caret_x(range.start);
            let x1 = self.unshaped_caret_x(range.end);
            out(Rect::new(x0, 0.0, x1 - x0, self.line_height()));
            return;
        };
        let lines = LineMap::new(buffer, self.text);
        let (start, end) = (lines.cursor(range.start), lines.cursor(range.end));
        for run in buffer.layout_runs() {
            push_run_selection_rects(&run, start, end, left, &mut out);
        }
    }
}

/// Where a caret sits inside a run: top-left in run-local pixels plus its visual
/// line's height as laid out (font fallback shifts ascent and descent), so it
/// matches the glyphs beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    /// Horizontal position in the block's local logical pixels.
    pub x: f32,
    /// Top of the caret, on the same axes.
    pub y_top: f32,
    /// Height of the line the caret sits on, as laid out.
    pub line_height: f32,
}

/// Where the caret on a zero-glyph line sits in a block `block_w` wide (cosmic
/// reports `x = 0`). Measured against the block, not the wrap width, or the
/// owner's alignment would apply twice.
fn empty_line_x(block_w: f32, halign: HAlign) -> f32 {
    match halign {
        HAlign::Center => block_w * 0.5,
        HAlign::Right => block_w,
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
    }
}

fn push_run_selection_rects(
    run: &cosmic_text::LayoutRun<'_>,
    cursor_start: cosmic_text::Cursor,
    cursor_end: cosmic_text::Cursor,
    left: f32,
    out: &mut impl FnMut(Rect),
) {
    // Runs outside the selected lines are rejected first, as cosmic-text's editor
    // does: the per-grapheme test below treats a run whose line differs from both
    // cursors as fully selected.
    if run.line_i < cursor_start.line || run.line_i > cursor_end.line {
        return;
    }
    let mut selected: Option<(f32, f32)> = None;
    let mut flush = |selected: &mut Option<(f32, f32)>| {
        if let Some((min_x, max_x)) = selected.take() {
            let width = max_x - min_x;
            if width > 0.0 {
                out(Rect::new(
                    min_x - left,
                    run.line_top,
                    width,
                    run.line_height,
                ));
            }
        }
    };

    for glyph in run.glyphs {
        let cluster = &run.text[glyph.start..glyph.end];
        let total = cluster.grapheme_indices(true).count().max(1);
        let grapheme_width = glyph.w / total as f32;
        let mut x = glyph.x;
        for (i, grapheme) in cluster.grapheme_indices(true) {
            let start = glyph.start + i;
            let end = start + grapheme.len();
            let is_selected = (cursor_start.line != run.line_i || end > cursor_start.index)
                && (cursor_end.line != run.line_i || start < cursor_end.index);
            if is_selected {
                selected = Some(match selected {
                    Some((min, max)) => (min.min(x), max.max(x + grapheme_width)),
                    None => (x, x + grapheme_width),
                });
            } else {
                flush(&mut selected);
            }
            x += grapheme_width;
        }
    }
    flush(&mut selected);
}

/// Byte offsets into a run's source text and cosmic `Cursor`s into its buffer,
/// mapped through the buffer's own lines (split at `\n`, `\r`, `\r\n`, `\n\r`).
/// A truncated run shapes `prefix + "…"`, so the shown text matches the source
/// only up to [`Self::shown`]; an offset past it maps to its end.
#[derive(Debug)]
struct LineMap<'a> {
    buffer: &'a cosmic_text::Buffer,
    /// Source bytes the buffer shows from the start; always a char boundary.
    shown: usize,
}

impl<'a> LineMap<'a> {
    fn new(buffer: &'a cosmic_text::Buffer, text: &str) -> Self {
        let source = text.as_bytes();
        let mut shown = 0;
        'lines: for line in &buffer.lines {
            for part in [line.text(), line.ending().as_str()] {
                let common = part
                    .bytes()
                    .zip(&source[shown..])
                    .take_while(|(a, b)| a == *b)
                    .count();
                shown += common;
                if common < part.len() {
                    break 'lines;
                }
            }
        }
        while !text.is_char_boundary(shown) {
            shown -= 1;
        }
        Self { buffer, shown }
    }

    /// The cursor at `byte_offset`, clamped to what the buffer shows.
    fn cursor(&self, byte_offset: usize) -> cosmic_text::Cursor {
        let byte_offset = byte_offset.min(self.shown);
        let last = self.buffer.lines.len().saturating_sub(1);
        let mut start = 0;
        for (index, line) in self.buffer.lines.iter().enumerate() {
            let len = line.text().len();
            let next = start + len + line.ending().as_str().len();
            if byte_offset < next || index == last {
                return cosmic_text::Cursor::new(index, (byte_offset - start).min(len));
            }
            start = next;
        }
        cosmic_text::Cursor::new(0, 0)
    }

    fn byte(&self, cursor: cosmic_text::Cursor) -> usize {
        let mut start = 0;
        for line in self.buffer.lines.iter().take(cursor.line) {
            start += line.text().len() + line.ending().as_str().len();
        }
        let len = self
            .buffer
            .lines
            .get(cursor.line)
            .map_or(0, |line| line.text().len());
        (start + cursor.index.min(len)).min(self.shown)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use super::*;

    impl TextProbe<'_> {
        /// Raw shaped buffer for in-tree cross-checks; test-only so no cosmic types leak.
        pub(crate) fn buffer_for_test(&self) -> Option<&cosmic_text::Buffer> {
            self.shaped().map(|shaped| shaped.buffer)
        }

        /// The key a caller can replay this run through, or `None` where no buffer backs it.
        pub(crate) fn shaped_key(&self) -> Option<TextShapeKey> {
            self.shaped().and(self.key)
        }
    }

    /// The byte/`Cursor` mapping over a shaped `buffer`, for `text::tests::geometry`.
    pub(crate) fn cursor_from_byte(
        buffer: &cosmic_text::Buffer,
        text: &str,
        byte_offset: usize,
    ) -> cosmic_text::Cursor {
        LineMap::new(buffer, text).cursor(byte_offset)
    }

    pub(crate) fn cursor_to_byte(
        buffer: &cosmic_text::Buffer,
        text: &str,
        cursor: cosmic_text::Cursor,
    ) -> usize {
        LineMap::new(buffer, text).byte(cursor)
    }
}

#[cfg(test)]
mod tests;
