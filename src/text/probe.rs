//! Read-only geometry over one shaped text layout: caret placement,
//! pixel hit-testing, and selection rects. Consumed by `TextEdit` —
//! never by the shaping hot path. Placing a measured block inside its
//! leaf rect is plain box alignment with no text state, so it lives with
//! `Align` as [`crate::layout::types::align::align_in_rect`].

use crate::layout::types::align::HAlign;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::mono;
use crate::text::{ShaperInner, TextShapeRequest};
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
    /// Extent of the shaped run. `Size::ZERO` for empty text.
    pub(crate) size: Size,
    pub(crate) request: TextShapeRequest<'t>,
    inner: RefMut<'s, ShaperInner>,
}

impl<'s, 't> TextLayoutProbe<'s, 't> {
    pub(super) fn new(
        size: Size,
        request: TextShapeRequest<'t>,
        inner: RefMut<'s, ShaperInner>,
    ) -> Self {
        Self {
            size,
            request,
            inner,
        }
    }

    /// Shaped buffer behind this layout; `None` on the gated mono metric
    /// (no cosmic to ask) and for empty text (an unshaped request).
    fn buffer(&self) -> Option<&cosmic_text::Buffer> {
        self.inner.cosmic.as_ref()?.buffer_for(self.request.key)
    }
    /// (x, y_top, line_height) for the caret at `byte_offset`.
    /// Multi-line aware via cosmic-text layout runs (each `\n` and each
    /// soft-wrap segment becomes a distinct visual line). Mono fallback /
    /// empty-text path collapses to a 1D layout — `y_top = 0`, `x` from a
    /// flat mono per-byte estimate — usable for tests / headless.
    ///
    /// Horizontal placement defers to `LayoutRun::cursor_position`, the
    /// same geometry `Buffer::hit` inverts, so a hit-test → caret round
    /// trip lands back where it started. It resolves the two cases a
    /// glyph-start scan cannot: an RTL glyph carries the caret at its
    /// right edge, and an offset interior to a ligature or Indic cluster
    /// interpolates across the cluster instead of jumping to its far end.
    pub(crate) fn cursor_xy(&self, byte_offset: usize) -> CursorPos {
        let line_height_px = self.request.key.line_height_px();
        let max_width_px = self.request.key.max_width_px();
        let halign = self.request.key.halign();
        let target = cursor_from_byte(self.request.text, byte_offset);
        let Some(buffer) = self.buffer() else {
            return CursorPos {
                x: mono::caret_x(
                    self.request.text,
                    byte_offset,
                    self.request.key.font_size_px(),
                    empty_line_x(max_width_px, halign),
                ),
                y_top: 0.0,
                line_height: line_height_px,
            };
        };

        let mut last_in_line: Option<CursorPos> = None;
        for run in buffer.layout_runs() {
            if run.line_i != target.line {
                continue;
            }
            // A glyphless visual line has nothing to hang the caret on and
            // cosmic reports x = 0; place it where per-line align will put
            // the first typed glyph instead.
            let placed = if run.glyphs.is_empty() {
                Some(empty_line_x(max_width_px, halign))
            } else {
                run.cursor_position(&target)
            };
            let x = placed.unwrap_or_else(|| run.glyphs.last().map_or(0.0, |g| g.x + g.w));
            let pos = CursorPos {
                x,
                y_top: run.line_top,
                line_height: run.line_height,
            };
            if placed.is_some() {
                return pos;
            }
            // Soft wrap splits one logical line across runs, so a miss here
            // just means the offset belongs to a later run.
            last_in_line = Some(pos);
        }
        last_in_line.unwrap_or(CursorPos {
            x: 0.0,
            y_top: 0.0,
            line_height: line_height_px,
        })
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
            // No shaped buffer to wash: mono lays the band out 1D, and
            // empty text collapses it to nothing.
            let font_size_px = self.request.key.font_size_px();
            let x0 = mono::caret_x(self.request.text, range.start, font_size_px, 0.0);
            let x1 = mono::caret_x(self.request.text, range.end, font_size_px, 0.0);
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
fn empty_line_x(max_width_px: Option<f32>, halign: HAlign) -> f32 {
    let Some(w) = max_width_px else { return 0.0 };
    match halign {
        HAlign::Center => w * 0.5,
        HAlign::Right => w,
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
    }
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
pub(in crate::text) fn cursor_from_byte(text: &str, byte_offset: usize) -> cosmic_text::Cursor {
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
pub(in crate::text) fn cursor_to_byte(text: &str, cursor: cosmic_text::Cursor) -> usize {
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
mod internals {
    use crate::text::probe::TextLayoutProbe;

    impl TextLayoutProbe<'_, '_> {
        /// Raw shaped buffer, reach-in for the in-tree cross-checks
        /// against cosmic's own geometry (`run.highlight`). Test-only
        /// so production builds expose no cosmic types outside
        /// `src/text/`.
        pub(in crate::text) fn buffer_for_test(&self) -> Option<&cosmic_text::Buffer> {
            self.buffer()
        }
    }
}
