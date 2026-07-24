//! Read-only geometry over one shaped text layout: caret placement,
//! pixel hit-testing, selection rects, and aligned placement of a
//! measured block inside its leaf rect. Consumed by `TextEdit`, the
//! cascade, and the encoder — never by the shaping hot path.

use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::mono;
use crate::text::{ShaperInner, TextMeasurement, TextShapeRequest};
use std::cell::RefMut;
use unicode_segmentation::UnicodeSegmentation;

/// Output buffer for [`TextLayoutProbe::selection_rects`]. Stores selections
/// up to 16 visual lines inline; larger selections retain their spill
/// allocation when the caller reuses the buffer.
pub(crate) const SELECTION_RECTS_INLINE_CAPACITY: usize = 16;
pub(crate) type SelectionRects = tinyvec::TinyVec<[Rect; SELECTION_RECTS_INLINE_CAPACITY]>;

/// One shaped text layout leased for read-only geometry queries,
/// minted by `TextShaper::layout`. Holds the shaper's exclusive
/// `RefCell` borrow until dropped — re-entering the shaper while a
/// probe is alive is a logic error the `RefCell` catches at runtime.
#[derive(Debug)]
pub(crate) struct TextLayoutProbe<'s, 't> {
    pub(crate) measurement: TextMeasurement,
    pub(crate) request: TextShapeRequest<'t>,
    inner: RefMut<'s, ShaperInner>,
}

impl<'s, 't> TextLayoutProbe<'s, 't> {
    pub(crate) fn new(
        measurement: TextMeasurement,
        request: TextShapeRequest<'t>,
        inner: RefMut<'s, ShaperInner>,
    ) -> Self {
        Self {
            measurement,
            request,
            inner,
        }
    }

    /// Shaped buffer behind this layout; `None` on the test-only mono
    /// fallback and for empty text (`TextShapeKey::INVALID` keys).
    fn buffer(&self) -> Option<&cosmic_text::Buffer> {
        self.inner.cosmic.as_ref()?.buffer_for(self.measurement.key)
    }
    /// (x, y_top, line_height) for the caret at `byte_offset`.
    /// Multi-line aware via cosmic-text layout runs (each `\n` and each
    /// soft-wrap segment becomes a distinct visual line). Mono fallback /
    /// empty-text path collapses to a 1D layout — `y_top = 0`, `x` from a
    /// flat mono per-byte estimate — usable for tests / headless.
    pub(crate) fn cursor_xy(&self, byte_offset: usize) -> CursorPos {
        let font_size_px = self.request.key.font_size_px();
        let line_height_px = self.request.key.line_height_px();
        let max_width_px = self.request.key.max_width_px();
        let halign = self.request.key.halign();
        let target = cursor_from_byte(self.request.text, byte_offset);
        let Some(buffer) = self.buffer() else {
            let x = if self.request.text.is_empty() {
                empty_line_x(max_width_px, halign)
            } else {
                mono::caret_x_single_line(self.request.text, byte_offset, font_size_px)
            };
            return CursorPos {
                x,
                y_top: 0.0,
                line_height: line_height_px,
            };
        };

        let mut last_in_line: Option<(f32, f32, f32)> = None;
        for run in buffer.layout_runs() {
            if run.line_i != target.line {
                continue;
            }
            let line_end_x = run
                .glyphs
                .last()
                .map(|g| g.x + g.w)
                .unwrap_or_else(|| empty_line_x(max_width_px, halign));
            last_in_line = Some((line_end_x, run.line_top, run.line_height));
            for glyph in run.glyphs {
                if glyph.start == target.index {
                    return CursorPos {
                        x: glyph.x,
                        y_top: run.line_top,
                        line_height: run.line_height,
                    };
                }
                if glyph.start < target.index && target.index < glyph.end {
                    return CursorPos {
                        x: glyph.x + glyph.w,
                        y_top: run.line_top,
                        line_height: run.line_height,
                    };
                }
            }
        }
        let (line_end_x, line_top, line_height) =
            last_in_line.unwrap_or((0.0, 0.0, line_height_px));
        CursorPos {
            x: line_end_x,
            y_top: line_top,
            line_height,
        }
    }

    /// Pixel-position → byte-offset. Multi-line aware on the cosmic
    /// path via `Buffer::hit`. Mono / empty-text falls back to a 1D
    /// `(x ÷ 0.5·font_size)` scan over char boundaries — enough for
    /// headless single-line click tests, ignores `y` entirely.
    pub(crate) fn byte_at_xy(&self, x: f32, y: f32) -> usize {
        match self.buffer() {
            Some(buffer) => buffer
                .hit(x, y)
                .map(|cursor| cursor_to_byte(self.request.text, cursor))
                .unwrap_or(self.request.text.len()),
            None => mono::byte_at_x(self.request.text, x, self.request.key.font_size_px()),
        }
    }

    pub(crate) fn selection_rects(&self, range: std::ops::Range<usize>, out: &mut SelectionRects) {
        out.clear();
        if range.is_empty() {
            return;
        }
        let Some(buffer) = self.buffer() else {
            let font_size_px = self.request.key.font_size_px();
            let x0 = mono::caret_x_single_line(self.request.text, range.start, font_size_px);
            let x1 = mono::caret_x_single_line(self.request.text, range.end, font_size_px);
            out.push(Rect::new(
                x0,
                0.0,
                x1 - x0,
                self.request.key.line_height_px(),
            ));
            return;
        };
        let start = cursor_from_byte(self.request.text, range.start);
        let end = cursor_from_byte(self.request.text, range.end);
        for run in buffer.layout_runs() {
            push_run_selection_rects(&run, start, end, out);
        }
    }
}

/// Caret position returned by [`TextLayoutProbe::cursor_xy`].
/// Top-left in text-local pixels plus the visual line's height (so the
/// renderer can size the caret rect to match the line cosmic-text laid
/// out, not the requested `line_height_px` — they differ when font
/// fallback shifts ascent/descent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorPos {
    pub(crate) x: f32,
    pub(crate) y_top: f32,
    pub(crate) line_height: f32,
}

/// Where the caret on a zero-glyph line ends up after cosmic's
/// per-line align. Mirrors cosmic's `(line_width - line_w) * factor`
/// formula collapsed for `line_w = 0`. Used by `cursor_xy` when the
/// shaped buffer is missing (empty buffer / mono fallback) — without
/// it, an empty right-aligned multi-line editor would paint its
/// caret at `x = 0` instead of at the right edge.
pub(crate) fn empty_line_x(max_width_px: Option<f32>, halign: HAlign) -> f32 {
    let Some(w) = max_width_px else { return 0.0 };
    match halign {
        HAlign::Center => w * 0.5,
        HAlign::Right => w,
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
    }
}

/// Position a measured text block inside `leaf` per `align`: `min`
/// shifted by the alignment offset, `size` = the measured bbox (the
/// composer takes `min` as the glyph origin and `size` as the clip
/// bounds). Glyphs don't stretch, so `Auto`/`Stretch` collapse to
/// start — matches arrange-axis placement for non-stretchable content — and
/// overflow on an axis clamps that axis's offset to zero so oversized
/// text pins to the leading edge.
///
/// Coordinate-system agnostic: the cascade and encoder pass
/// owner-local / screen-space leaf rects; `TextEdit` passes a
/// zero-origin rect and reads `.min` back as the bare offset for its
/// caret/selection math. One definition for all of them — glyphs,
/// caret, and selection wash must shift by the same offset or the
/// caret drifts off its glyph.
pub(crate) fn text_in_rect(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
}

// `LayoutRun::highlight` builds a temporary `Vec` per run, so stream its spans directly.
fn push_run_selection_rects(
    run: &cosmic_text::LayoutRun<'_>,
    cursor_start: cosmic_text::Cursor,
    cursor_end: cosmic_text::Cursor,
    out: &mut SelectionRects,
) {
    // The per-grapheme test below (ported from `LayoutRun::highlight`) treats a
    // run whose line differs from both cursors as fully selected, so runs on
    // lines outside the selected range must be rejected up front — the same
    // guard cosmic-text's editor applies before calling `highlight`.
    if run.line_i < cursor_start.line || run.line_i > cursor_end.line {
        return;
    }
    let mut selected: Option<(f32, f32)> = None;
    let mut flush = |selected: &mut Option<(f32, f32)>| {
        if let Some((min_x, max_x)) = selected.take() {
            let width = max_x - min_x;
            if width > 0.0 {
                out.push(Rect::new(min_x, run.line_top, width, run.line_height));
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

/// Map a UTF-8 byte offset into `text` to a cosmic-text `Cursor`:
/// `line` = count of `\n` before the offset, `index` = bytes since
/// the most recent `\n` (or start of text).
pub(crate) fn cursor_from_byte(text: &str, byte_offset: usize) -> cosmic_text::Cursor {
    let prefix = &text.as_bytes()[..byte_offset.min(text.len())];
    let line = prefix.iter().filter(|&&b| b == b'\n').count();
    let line_start = prefix
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    cosmic_text::Cursor::new(line, byte_offset - line_start)
}

/// Inverse of [`cursor_from_byte`]. Walks `text` to find the
/// `line`-th `\n` and adds `cursor.index`.
pub(crate) fn cursor_to_byte(text: &str, cursor: cosmic_text::Cursor) -> usize {
    let line_start = if cursor.line == 0 {
        0
    } else {
        match text.match_indices('\n').nth(cursor.line - 1) {
            Some((i, _)) => i + 1,
            None => return text.len(),
        }
    };
    (line_start + cursor.index).min(text.len())
}

#[cfg(test)]
mod test_support {
    use crate::text::probe::TextLayoutProbe;

    impl TextLayoutProbe<'_, '_> {
        /// Raw shaped buffer, reach-in for the in-tree cross-checks
        /// against cosmic's own geometry (`run.highlight`). Test-only
        /// so production builds expose no cosmic types outside
        /// `src/text/`.
        pub(crate) fn buffer_for_test(&self) -> Option<&cosmic_text::Buffer> {
            self.buffer()
        }
    }
}
