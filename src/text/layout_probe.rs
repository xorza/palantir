//! Read-only geometry over one shaped text layout: caret placement,
//! pixel hit-testing, and selection rects. Reached by widgets through
//! [`TextProbe`](crate::text::probe::TextProbe) — never by the shaping
//! hot path. Placing a measured block inside its leaf rect is plain box
//! alignment with no text state, so it lives with `Align` as
//! [`crate::layout::types::align::align_in_rect`].

use crate::layout::types::align::HAlign;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::cosmic::ShapedRun;
use crate::text::request::TextShapeRequest;
use crate::text::shaper::ShaperInner;
use std::cell::RefMut;
use unicode_segmentation::UnicodeSegmentation;

/// One shaped text layout leased for read-only geometry queries,
/// minted by [`TextShaper::layout`](crate::text::shaper::TextShaper).
/// Holds the shaper's exclusive `RefCell` borrow until dropped —
/// re-entering the shaper while a probe is alive is a logic error the
/// `RefCell` catches at runtime.
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

    /// Shaped run behind this layout; `None` on the gated mono metric
    /// (no cosmic to ask) and for empty text (an unshaped request).
    ///
    /// Every query below reports in *block-local* coordinates — the same
    /// space [`Self::size`] measures and the encoder places — so each one
    /// takes [`ShapedRun::left`] off the buffer's own x, or adds it back
    /// when going the other way.
    fn shaped(&self) -> Option<ShapedRun<'_>> {
        self.inner.cosmic.as_ref()?.shaped_run(self.request.key)
    }

    /// Caret-x for a layout with no shaped buffer.
    ///
    /// Production reaches this only for empty text, whose block is
    /// zero-width — the only position in it is its own origin, and the
    /// owner is what places that block against an alignment. The gated
    /// mono metric also lands here, with real text, which is the arm
    /// `mono` answers — reached by full path because the module it lives
    /// in is gated away in a production build.
    ///
    /// Anything else is a wiring bug, not a case to answer with a
    /// plausible zero: [`TextShaper::layout`](crate::text::shaper::TextShaper)
    /// shapes non-empty text before minting the probe, and the probe
    /// holds the shaper's borrow for its whole life, so no sweep can
    /// evict that buffer out from under it.
    fn unshaped_caret_x(&self, byte_offset: usize) -> f32 {
        #[cfg(any(test, feature = "internals"))]
        if !self.request.text.is_empty() {
            return crate::text::mono::single_line_caret_x(
                self.request.text,
                byte_offset,
                self.request.key.font_size_px(),
            );
        }
        assert!(
            self.request.text.is_empty(),
            "a non-empty run with no shaped buffer requires the mono metric \
             (caret at byte {byte_offset})",
        );
        0.0
    }

    /// [`Self::unshaped_caret_x`]'s inverse, and unreachable on the same
    /// terms. Empty text has exactly one position, so production always
    /// answers 0.
    fn unshaped_byte_at(&self, target_x: f32) -> usize {
        #[cfg(any(test, feature = "internals"))]
        if !self.request.text.is_empty() {
            return crate::text::mono::nearest_byte(
                self.request.text,
                target_x,
                self.request.key.font_size_px(),
            );
        }
        assert!(
            self.request.text.is_empty(),
            "a non-empty run with no shaped buffer requires the mono metric \
             (hit-test at x {target_x})",
        );
        0
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
    pub(crate) fn cursor_xy(&self, byte_offset: usize) -> Caret {
        let line_height_px = self.request.key.line_height_px();
        let halign = self.request.key.halign();
        let target = cursor_from_byte(self.request.text, byte_offset);
        let Some(ShapedRun { buffer, left }) = self.shaped() else {
            // No shaped buffer means empty text (block-local x is 0, and
            // the owner aligns the empty block itself) or the mono metric.
            return Caret {
                x: self.unshaped_caret_x(byte_offset),
                y_top: 0.0,
                line_height: line_height_px,
            };
        };

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
            // A glyphless visual line has nothing to hang the caret on and
            // cosmic reports x = 0; place it where per-line align will put
            // the first typed glyph instead. That answer is block-local
            // already, so it skips the correction the buffer's own x needs.
            if run.glyphs.is_empty() {
                return Caret {
                    x: empty_line_x(self.size.w, halign),
                    y_top: run.line_top,
                    line_height: run.line_height,
                };
            }
            match run.cursor_position(&target) {
                Some(x) => return at(x),
                // Soft wrap splits one logical line across runs, so a miss
                // here just means the offset belongs to a later run.
                None => {
                    last_in_line = Some(at(run.glyphs.last().map_or(0.0, |g| g.x + g.w)));
                }
            }
        }
        last_in_line.unwrap_or(Caret {
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
        match self.shaped() {
            // `x` arrives block-local, so put it back into buffer space
            // before asking cosmic — the exact inverse of what
            // [`Self::cursor_xy`] subtracts, which is what keeps the
            // hit-test → caret round trip landing where it started.
            Some(ShapedRun { buffer, left }) => buffer
                .hit(x + left, y)
                .map(|cursor| cursor_to_byte(self.request.text, cursor))
                .unwrap_or(self.request.text.len()),
            None => self.unshaped_byte_at(x),
        }
    }

    /// Stream the rects covering `range`, one per visual line, into
    /// `out`.
    ///
    /// A sink rather than a container: the only caller that retains them
    /// owns a reused buffer, and materializing one here would allocate
    /// per frame for any selection past the inline capacity — which the
    /// `long_multiline_selection_alloc_free` audit fails on.
    pub(crate) fn selection_rects(
        &self,
        range: std::ops::Range<usize>,
        out: &mut impl FnMut(Rect),
    ) {
        if range.is_empty() {
            return;
        }
        let Some(ShapedRun { buffer, left }) = self.shaped() else {
            // No shaped buffer to wash: mono lays the band out 1D, and
            // empty text collapses it to nothing.
            let x0 = self.unshaped_caret_x(range.start);
            let x1 = self.unshaped_caret_x(range.end);
            out(Rect::new(
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
            push_run_selection_rects(&run, start, end, left, out);
        }
    }
}

/// Where a caret sits inside a run: top-left in run-local pixels, plus
/// the height of the visual line it landed on.
///
/// That height is what the line was actually laid out at, which is not
/// always the requested `line_height_px` — font fallback shifts ascent
/// and descent — so a caret sized from this matches the glyphs beside it
/// rather than the metric that was asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    pub x: f32,
    pub y_top: f32,
    pub line_height: f32,
}

/// Where the caret on a zero-glyph line sits inside a block `block_w`
/// wide. Cosmic reports `x = 0` for a glyphless line whatever the
/// alignment, so without this an empty line in a right-aligned block
/// would take its caret to the block's left edge instead of the edge the
/// first typed glyph will land on.
///
/// Measured against the *block*, not the wrap width: the block is what
/// the owner aligns inside the leaf rect, so aligning against the wrap
/// width here would place the caret as if that alignment happened twice.
fn empty_line_x(block_w: f32, halign: HAlign) -> f32 {
    match halign {
        HAlign::Center => block_w * 0.5,
        HAlign::Right => block_w,
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
    }
}

// `LayoutRun::highlight` builds a temporary `Vec` per run, so stream its spans directly.
fn push_run_selection_rects(
    run: &cosmic_text::LayoutRun<'_>,
    cursor_start: cosmic_text::Cursor,
    cursor_end: cosmic_text::Cursor,
    left: f32,
    out: &mut impl FnMut(Rect),
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

/// Map a UTF-8 byte offset into `text` to a cosmic-text `Cursor`:
/// `line` = count of `\n` before the offset, `index` = bytes since
/// the most recent `\n` (or start of text).
pub(super) fn cursor_from_byte(text: &str, byte_offset: usize) -> cosmic_text::Cursor {
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
pub(super) fn cursor_to_byte(text: &str, cursor: cosmic_text::Cursor) -> usize {
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
    use crate::text::layout_probe::TextLayoutProbe;

    impl TextLayoutProbe<'_, '_> {
        /// Raw shaped buffer, reach-in for the in-tree cross-checks
        /// against cosmic's own geometry (`run.highlight`). Test-only
        /// so production builds expose no cosmic types outside
        /// `src/text/`.
        pub(crate) fn buffer_for_test(&self) -> Option<&cosmic_text::Buffer> {
            self.shaped().map(|shaped| shaped.buffer)
        }
    }
}
