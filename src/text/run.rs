//! The public text-geometry surface: describe a run, probe its geometry.
//!
//! Layout already measures and paints text for any widget that records a
//! [`Shape::Text`](crate::Shape) — nothing here is needed for that. What
//! this module adds is the other direction: mapping between byte offsets
//! and positions inside a run, which is what a widget needs to place a
//! caret, turn a click into an offset, or paint a selection.
//!
//! Nothing shaped escapes `src/text/`: [`TextProbe`] answers in plain
//! geometry, and the cosmic-text buffers behind it stay private.

use std::ops::Range;

use crate::layout::types::align::Align;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::probe::{Caret, TextLayoutProbe};
use crate::text::wrap::TextWrap;
use crate::text::{FontFamily, FontWeight, TextShapeRequest};

/// One text run, described the way [`Shape::Text`](crate::Shape)
/// describes one.
///
/// **The spelling mirrors `Shape::Text` on purpose.** A probe that
/// describes a different run than the paint puts the caret in the wrong
/// place, and that is invisible until someone reports their cursor
/// sitting slightly off in bold text — so the two are written the same
/// way, field for field, and a mismatch reads as one in review.
///
/// The paint-only fields are absent (`color`, `local_origin`): they move
/// glyphs on screen but never change how the run is shaped, so they
/// cannot change any answer here.
#[derive(Clone, Copy, Debug)]
pub struct TextRun<'a> {
    pub text: &'a str,
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub wrap: TextWrap,
    /// Only the horizontal half is read — cosmic lays out per-line `x`
    /// offsets from it, so it changes the shaped result. The vertical
    /// half places the block within its owner and is the encoder's
    /// business, exactly as in `Shape::Text`.
    pub align: Align,
    pub family: FontFamily,
    pub weight: FontWeight,
    /// The width the run is shaped against, or `None` for unbounded.
    ///
    /// The one field `Shape::Text` has no counterpart for: a painted run
    /// gets its width from the arranged rect, which does not exist until
    /// layout has run. A probe has to say which width it means — pass
    /// the inner width the run will be (or was) laid out in.
    ///
    /// Ignored for the [`TextWrap`] policies that always keep their
    /// unbounded shape, so passing a width to a `SingleLine` run is not
    /// a mistake, just inert.
    pub max_width_px: Option<f32>,
}

impl<'a> TextRun<'a> {
    /// Lower to the shaper's request.
    ///
    /// Bounded exactly when layout would bind it — same
    /// [`TextWrap::line_fit`] mapping, same `halign` — so probing a run
    /// hits the buffer the paint shaped rather than minting a second one
    /// that answers slightly differently.
    pub(crate) fn request(&self) -> TextShapeRequest<'a> {
        let request = TextShapeRequest::unbounded(
            self.text,
            self.font_size_px,
            self.line_height_px,
            self.family,
            self.weight,
        );
        match (self.max_width_px, self.wrap.line_fit()) {
            (Some(width), Some(fit)) => request.bounded(width, self.align.halign(), fit),
            _ => request,
        }
    }
}

/// Geometry queries over one shaped run, minted by
/// [`Ui::probe_text`](crate::Ui::probe_text).
///
/// **A live probe holds the shaper's exclusive borrow**, which is why it
/// borrows the `Ui` mutably: two overlapping probes are then E0499 at
/// compile time rather than a `RefCell` panic in someone's running app.
/// Two *sequential* probes are fine — end the first with a block, or let
/// a temporary drop at the end of its statement.
///
/// One lifetime, though the layout behind it tracks the shaper borrow and
/// the run's text separately: nothing here hands back a borrow of either,
/// so collapsing them costs nothing and spares every caller a second
/// `'_`.
#[derive(Debug)]
pub struct TextProbe<'a> {
    inner: TextLayoutProbe<'a, 'a>,
}

impl<'a> TextProbe<'a> {
    pub(crate) fn new(inner: TextLayoutProbe<'a, 'a>) -> Self {
        Self { inner }
    }

    /// Extent of the shaped run; `Size::ZERO` for empty text.
    pub fn size(&self) -> Size {
        self.inner.size
    }

    /// Where the caret sits at `byte_offset`.
    pub fn caret_at(&self, byte_offset: usize) -> Caret {
        self.inner.cursor_xy(byte_offset)
    }

    /// The byte offset a point lands on, in run-local coordinates —
    /// click-to-caret. Clamped to the run, so a point outside it answers
    /// the nearest end rather than nothing.
    pub fn byte_at(&self, x: f32, y: f32) -> usize {
        self.inner.byte_at_xy(x, y)
    }

    /// Every rect covering `range`, one per visual line, in run-local
    /// coordinates.
    ///
    /// A callback rather than a returned collection: the rects are
    /// consumed immediately (painted, or unioned) and a caller that
    /// wants to retain them can push into a buffer it already owns, so
    /// nothing here allocates per frame.
    pub fn selection_rects(&self, range: Range<usize>, mut f: impl FnMut(Rect)) {
        self.inner.selection_rects(range, &mut f);
    }

    /// 64-bit hash of the run's text, as the shaping cache keys it —
    /// for a caller comparing "is this the same string as last frame?"
    /// without retaining a copy of it.
    pub fn text_hash(&self) -> u64 {
        self.inner.request.key.text_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::harness::UiHarness;

    /// The whole public probe surface, against the mono shaper's exact
    /// metric: every glyph is `font_size_px * 0.5` wide, so at 16 px a
    /// character is 8 px and every expected value below is arithmetic
    /// rather than a recorded observation.
    ///
    /// This is deliberately written the way a caller's own widget would
    /// have to write it — `Ui::probe_text` and nothing else. If the
    /// public surface stops being enough to place a caret, hit-test a
    /// click, or wash a selection, it stops compiling here.
    #[test]
    fn probing_a_run_maps_bytes_and_positions_both_ways() {
        const EM: f32 = 8.0; // 16 px font, mono half-width advance.
        let mut harness = UiHarness::arena();
        let ui = harness.ui();

        fn run(text: &str, max_width_px: Option<f32>) -> TextRun<'_> {
            TextRun {
                text,
                font_size_px: 16.0,
                line_height_px: 20.0,
                wrap: TextWrap::SingleLine,
                align: Align::LEFT,
                family: FontFamily::Sans,
                weight: FontWeight::Regular,
                max_width_px,
            }
        }

        {
            let probe = ui.probe_text(run("hello", None));
            // 5 glyphs × 8 px.
            assert_eq!(probe.size().w, 5.0 * EM, "run width is 5 mono glyphs");

            // Caret sits on glyph boundaries: byte n → n × 8 px.
            for byte in 0..=5 {
                assert_eq!(
                    probe.caret_at(byte).x,
                    byte as f32 * EM,
                    "caret at byte {byte}",
                );
            }

            // …and the inverse rounds to the nearest boundary, so the
            // two halves of a glyph fall to opposite sides: 3 px into an
            // 8 px cell is still byte 0, 5 px is byte 1.
            assert_eq!(probe.byte_at(3.0, 0.0), 0, "left half of glyph 0");
            assert_eq!(probe.byte_at(5.0, 0.0), 1, "right half of glyph 0");
            assert_eq!(probe.byte_at(3.0 * EM, 0.0), 3, "exactly on a boundary");
            assert_eq!(probe.byte_at(999.0, 0.0), 5, "past the end clamps");

            // Selection 1..4 is three glyphs starting one in: x = 8,
            // w = 24, on the run's single line.
            let mut rects = Vec::new();
            probe.selection_rects(1..4, |rect| rects.push(rect));
            assert_eq!(rects.len(), 1, "one visual line, one rect");
            assert_eq!(rects[0].min.x, EM);
            assert_eq!(rects[0].size.w, 3.0 * EM);

            // An empty range washes nothing at all.
            let mut none = Vec::new();
            probe.selection_rects(2..2, |rect| none.push(rect));
            assert!(none.is_empty(), "an empty selection has no rects");
        }

        // A `SingleLine` run keeps its unbounded shape, so handing it a
        // width changes nothing. This is the `TextWrap::line_fit`
        // mapping holding: bind the width for a policy that never wraps
        // and the probe would key a *different* shaped buffer than the
        // paint, which is how a caret ends up off by a few pixels.
        let bounded = ui.probe_text(run("hello", Some(16.0))).size().w;
        assert_eq!(bounded, 5.0 * EM, "a width is inert on SingleLine");

        // The hash discriminates content, which is what a caller compares
        // frame to frame instead of retaining the string.
        let a = ui.probe_text(run("hello", None)).text_hash();
        let b = ui.probe_text(run("hello", None)).text_hash();
        let c = ui.probe_text(run("hellO", None)).text_hash();
        assert_eq!(a, b, "same text, same hash");
        assert_ne!(a, c, "one changed byte changes the hash");
    }

    /// Wrapping runs bind their width, so the same text at the same size
    /// answers with a different height once it has to reflow — the other
    /// side of the `line_fit` mapping above.
    #[test]
    fn a_wrapping_run_binds_its_width_and_a_single_line_run_does_not() {
        let mut harness = UiHarness::arena();
        let ui = harness.ui();
        let run = |wrap, max_width_px| TextRun {
            text: "hello world",
            font_size_px: 16.0,
            line_height_px: 20.0,
            wrap,
            align: Align::LEFT,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
            max_width_px,
        };

        // 11 glyphs × 8 px = 88 px on one line, whatever width is offered.
        let single = ui.probe_text(run(TextWrap::SingleLine, Some(40.0))).size();
        assert_eq!(single.w, 88.0);

        // The same text told to wrap at 40 px cannot be 88 px wide.
        let wrapped = ui.probe_text(run(TextWrap::Wrap, Some(40.0))).size();
        assert!(
            wrapped.w <= 40.0,
            "a wrapping run fits its bound, got {}",
            wrapped.w,
        );
        assert!(
            wrapped.h > single.h,
            "and reflows onto more lines ({} vs {})",
            wrapped.h,
            single.h,
        );
    }
}
