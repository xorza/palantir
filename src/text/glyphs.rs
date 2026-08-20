//! Laying out and rasterizing glyphs for a caller that draws its own text.
//!
//! Palantir renders text for the widgets it owns; a [`GpuView`](crate::GpuView)
//! draws into its own target with its own pipelines, and palantir never sees
//! what is in it. Text inside one — a label pinned to a point in a 3D scene, a
//! dimension on a drawing — therefore has to be drawn by the view, which needs
//! the two things palantir's own text backend needs: where each glyph sits, and
//! what each glyph looks like.
//!
//! This is that pair, and nothing else. The atlas, the pipeline and the
//! blending are the caller's: what is shared is the font stack, which is the
//! part worth sharing — the platform's fonts are scanned once, and a view
//! drawing text in the same faces as the UI around it is not a coincidence to
//! be arranged but a consequence of asking the same shaper.

use crate::primitives::size::Size;
use crate::primitives::urect::URect;
use crate::text::cosmic::CosmicMeasure;
use crate::text::glyph_font::GlyphFont;
use crate::text::render::{GlyphImage, GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;
use crate::text::wrap::WrapFloor;
use glam::Vec2;
use std::cell::RefMut;

/// A lease on the shaper for laying out and rasterizing glyphs directly.
///
/// Holds the shaper's exclusive borrow until dropped, like every other way in,
/// so a caller keeps one for the length of a batch rather than per glyph — and
/// must not ask the same [`Ui`](crate::Ui) to measure text while holding one.
///
/// Minted by [`TextShaper::glyphs`](crate::TextShaper::glyphs), which a
/// `GpuView` reaches through [`GpuInitCtx`](crate::GpuInitCtx).
///
/// **Palantir's own text backend holds one too**, for the length of a batch's
/// encoded-cache misses — its `extract_glyphs` is the crate-facing half of
/// [`Self::line`], with the y-cull and the run placement a widget's own
/// scissor makes unnecessary. One lease type rather than two: the backend and
/// a `GpuView` want the same three answers off the same borrow, and a second
/// wrapper around it only restated the signatures. Cosmic types stay inside
/// `src/text/` because the field is private, not because a further type is
/// interposed.
#[derive(Debug)]
pub struct TextGlyphs<'a> {
    cosmic: RefMut<'a, CosmicMeasure>,
}

impl<'a> TextGlyphs<'a> {
    pub(super) fn new(cosmic: RefMut<'a, CosmicMeasure>) -> Self {
        Self { cosmic }
    }

    /// Resolve one run's glyphs at `placement`, restoring an evicted shaped
    /// buffer on the way. Returns whether any line was y-culled — a partial
    /// extraction must not become a renderer cache template.
    ///
    /// [`Self::line`] is this without the placement: a caller drawing into its
    /// own target positions the run itself and clips with its own scissor.
    pub(crate) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        self.cosmic.extract_glyphs(request, placement, out)
    }

    /// Lay `text` out as one unwrapped line at `scale`, rewriting `out` with a
    /// [`PlacedGlyph`] apiece.
    ///
    /// Positions are relative to the line's own origin — left edge, top of the
    /// line box — so a caller places the run by adding wherever it decided that
    /// origin is. That is what makes the answer reusable: text pinned to a
    /// moving point in a scene is laid out once and positioned every frame.
    ///
    /// Rasterization is binned to that origin, which is to say not binned at
    /// all: cosmic bins a run's fractional offset into each glyph's raster key,
    /// and a caller positioning glyphs in its own vertex shader has no
    /// fractional offset to declare at layout time. So one entry per glyph per
    /// size, rather than four, and subpixel phase is whatever the caller's own
    /// sampling makes of it.
    ///
    /// Rewrites rather than appends, so a caller laying out the same label every
    /// frame keeps one buffer.
    pub fn line(&mut self, text: &str, font: GlyphFont, scale: f32, out: &mut Vec<PlacedGlyph>) {
        // One of the two crate edges an empty run reaches — see
        // `TextShapeRequest`. Nothing to say shapes no buffer, and
        // extraction restores one rather than shaping it, so a run with no
        // request has no glyphs.
        let Some(request) = request(text, font) else {
            out.clear();
            return;
        };
        self.extract_glyphs(request, placement(scale), out);
    }

    /// How far `text` reaches when laid out in `font`, in the logical pixels
    /// `font` is sized in — without laying it out.
    ///
    /// What a caller anchoring text needs and cannot get from the glyphs: a
    /// run's advance is not the span of its bitmaps, since the last glyph's ink
    /// stops short of where the next one would start and a leading space has no
    /// ink at all.
    ///
    /// **Takes no raster scale, where [`TextGlyphs::line`] does.** A scale here
    /// could only multiply this extent, and that is not what `line` does with
    /// its own: it hands each glyph to cosmic's subpixel binning, which rounds
    /// every position at the raster size. The two therefore disagree by the
    /// rounding at any scale above 1 — so rather than take a `scale` that
    /// invites the two to be read as one measurement, this answers in logical
    /// pixels and leaves scaling to the caller that knows what it is for.
    /// Anchor from this; position glyphs from `line`.
    ///
    /// The shaper caches the shaped buffer, so asking this and then
    /// [`TextGlyphs::line`] for the same run shapes once.
    pub fn measure(&mut self, text: &str, font: GlyphFont) -> Size {
        // The measuring half of the same edge [`Self::line`] answers: a run
        // with nothing to shape reaches to nothing.
        request(text, font).map_or(Size::ZERO, |request| {
            self.cosmic.root(request, WrapFloor::Skip).size
        })
    }

    /// The bitmap for one glyph, or `None` where the face cannot produce an
    /// image for it.
    ///
    /// Uncached on palantir's side. The caller's atlas is the cache — this is
    /// the same call palantir's own text backend makes on an atlas miss, and
    /// caching it twice would be paying for a copy nobody reads.
    pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<GlyphImage> {
        self.cosmic.rasterize_glyph(glyph)
    }
}

/// One unwrapped line's shape request, or `None` where there is nothing
/// to shape.
fn request(text: &str, font: GlyphFont) -> Option<TextShapeRequest<'_>> {
    TextShapeRequest::unbounded(text, font)
}

/// The run placed at its own origin, culled against nothing.
///
/// A caller drawing into its own target clips with its own scissor, and the
/// y-cull here is a whole-line test against a rectangle this has no business
/// guessing at — so the bounds are the widest a `URect` states.
fn placement(scale: f32) -> RunPlacement {
    RunPlacement {
        origin: Vec2::ZERO,
        scale,
        bounds: URect::new(0, 0, u32::MAX, u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::render::GlyphImageKind;
    use crate::text::shaper::TextShaper;

    /// A real shaper, because the mono fallback shapes no buffers and has no
    /// faces to rasterize from — this API is the render side, and there is
    /// nothing of it to test against a measurer that only measures.
    fn shaper() -> TextShaper {
        TextShaper::new()
    }

    /// A run lays out one glyph per character, left to right, and lays out the
    /// same run identically twice.
    ///
    /// The repeat is the half an external atlas rests on: raster keys are what
    /// a caller's atlas keys its entries by, so a run whose keys moved between
    /// two identical calls would miss its own cache every frame.
    #[test]
    fn a_line_lays_out_left_to_right_and_repeats_itself() {
        let shaper = shaper();
        let mut glyphs = shaper.glyphs();
        let font = GlyphFont::new(16.0);

        let mut out = Vec::new();
        glyphs.line("abc", font, 1.0, &mut out);
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out[0].x < out[1].x && out[1].x < out[2].x, "{out:?}");

        let first: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
        // Into a buffer that already holds the answer: rewritten, not appended.
        glyphs.line("abc", font, 1.0, &mut out);
        let again: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
        assert_eq!(first, again);

        // Different text is different glyphs — the keys are about the glyph and
        // not about the request having been made.
        glyphs.line("xyz", font, 1.0, &mut out);
        let other: Vec<_> = out.iter().map(|glyph| glyph.raster_key).collect();
        assert_ne!(first, other);
    }

    /// Nothing to lay out lays out nothing, and reaches nowhere.
    #[test]
    fn an_empty_line_has_no_glyphs_and_no_extent() {
        let shaper = shaper();
        let mut glyphs = shaper.glyphs();
        let font = GlyphFont::new(16.0);

        let mut out = vec![];
        glyphs.line("a", font, 1.0, &mut out);
        assert!(!out.is_empty());

        glyphs.line("", font, 1.0, &mut out);
        assert!(out.is_empty(), "an empty run left glyphs behind: {out:?}");
        assert_eq!(glyphs.measure("", font), Size::ZERO);
    }

    /// The raster scale changes what is rasterized, not what is laid out.
    ///
    /// Both halves matter to a caller drawing on a HiDPI surface: it wants the
    /// same glyphs in the same order, rasterized bigger — and it wants the two
    /// sizes kept apart in its atlas, which is what the keys differing says.
    #[test]
    fn scale_changes_the_raster_and_not_the_run() {
        let shaper = shaper();
        let mut glyphs = shaper.glyphs();
        let font = GlyphFont::new(16.0);

        let mut single = Vec::new();
        glyphs.line("abc", font, 1.0, &mut single);
        let mut double = Vec::new();
        glyphs.line("abc", font, 2.0, &mut double);

        assert_eq!(single.len(), double.len());
        for (one, two) in single.iter().zip(&double) {
            assert_ne!(
                one.raster_key, two.raster_key,
                "two scales shared one raster"
            );
        }
        // Laid out twice as far across, because every advance is.
        assert!(double[2].x > single[2].x, "{single:?} {double:?}");

        // The measured extent is the run's own, in logical pixels — the raster
        // scale cannot reach it, which is why it takes none. What it *is* is
        // the advance, so the last glyph starts inside it: that is the whole
        // reason a caller anchors from `measure` and positions from `line`,
        // and the pair is only usable together while it holds.
        let measured = glyphs.measure("abc", font);
        assert!(measured.w > 0.0 && measured.h > 0.0);
        assert!(
            (single[2].x as f32) < measured.w,
            "the last glyph of {:?} starts at {} but the run measures {}",
            "abc",
            single[2].x,
            measured.w,
        );
    }

    /// A laid-out glyph rasterizes to a bitmap of exactly the size it claims.
    ///
    /// The end-to-end check on the pair: a key that came out of a layout goes
    /// back in and produces ink, which is the whole of what a caller's atlas
    /// needs and the one thing neither half can be asked on its own.
    #[test]
    fn a_placed_glyph_rasterizes_to_the_bitmap_it_describes() {
        let shaper = shaper();
        let mut glyphs = shaper.glyphs();

        let mut out = Vec::new();
        glyphs.line("A", GlyphFont::new(32.0), 1.0, &mut out);
        let [placed] = out[..] else {
            panic!("one letter laid out as {out:?}");
        };

        let image = glyphs
            .rasterize(placed.raster_key)
            .expect("a capital A has an image");
        assert_eq!(image.kind, GlyphImageKind::Mask);
        assert!(image.placement.width > 0 && image.placement.height > 0);
        // One byte of coverage per pixel, and the rows tightly packed — which
        // is what a caller blits into its atlas on.
        assert_eq!(
            image.data.len(),
            (image.placement.width * image.placement.height) as usize
        );
        assert!(
            image.data.iter().any(|&coverage| coverage > 0),
            "the glyph rasterized blank"
        );
    }
}
