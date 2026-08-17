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
use crate::text::render::{
    GlyphImage, GlyphRasterKey, PlacedGlyph, RunPlacement, TextRenderSession,
};
use crate::text::request::TextShapeRequest;
use crate::text::{FontFamily, FontWeight};
use glam::Vec2;

/// Which face to shape in, and how big.
///
/// Sizes are logical pixels; the raster scale is [`TextGlyphs::line`]'s, because
/// it is a property of the surface being drawn into rather than of the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphFont {
    pub size_px: f32,
    /// Leading is the caller's to choose. Palantir's own widgets derive one
    /// from the type scale; a single line pinned to a point in space has no
    /// stack to sit in, so this defaults to the size itself.
    pub line_height_px: f32,
    pub family: FontFamily,
    pub weight: FontWeight,
}

impl GlyphFont {
    /// `size_px` in the default family and weight, led at its own size.
    ///
    /// Every field is public, so anything else is a struct update over this —
    /// `GlyphFont { family: FontFamily::Mono, ..GlyphFont::new(16.0) }`, which
    /// holds in a `const` too.
    ///
    /// The two defaults are spelled out rather than asked of [`Default`],
    /// which a derive does not make a `const fn`. They are the `#[default]`
    /// variants of the enums beside them and have to stay that.
    pub const fn new(size_px: f32) -> Self {
        Self {
            size_px,
            line_height_px: size_px,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        }
    }
}

/// A lease on the shaper for laying out and rasterizing glyphs directly.
///
/// Holds the shaper's exclusive borrow until dropped, like every other way in,
/// so a caller keeps one for the length of a batch rather than per glyph — and
/// must not ask the same [`Ui`](crate::Ui) to measure text while holding one.
///
/// Minted by [`TextShaper::glyphs`](crate::TextShaper::glyphs), which a
/// `GpuView` reaches through [`GpuInitCtx`](crate::GpuInitCtx).
#[derive(Debug)]
pub struct TextGlyphs<'a> {
    session: TextRenderSession<'a>,
}

impl<'a> TextGlyphs<'a> {
    pub(crate) fn new(session: TextRenderSession<'a>) -> Self {
        Self { session }
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
        // Nothing to say shapes no buffer, and extraction restores one rather
        // than shaping it — so the empty run has to be answered here. Palantir's
        // own encoder never reaches this because it drops empty runs upstream,
        // which is what leaves the hot path free of the check.
        if text.is_empty() {
            out.clear();
            return;
        }
        self.session
            .extract_glyphs(request(text, font), placement(scale), out);
    }

    /// How far `text` reaches when laid out in `font` at `scale`, in physical
    /// pixels — without laying it out.
    ///
    /// What a caller anchoring text needs and cannot get from the glyphs: a
    /// run's advance is not the span of its bitmaps, since the last glyph's ink
    /// stops short of where the next one would start and a leading space has no
    /// ink at all.
    ///
    /// The shaper caches the shaped buffer, so asking this and then
    /// [`TextGlyphs::line`] for the same run shapes once.
    pub fn measure(&mut self, text: &str, font: GlyphFont, scale: f32) -> Size {
        let Size { w, h } = self.session.measure(request(text, font));
        Size {
            w: w * scale,
            h: h * scale,
        }
    }

    /// The bitmap for one glyph, or `None` where the face cannot produce an
    /// image for it.
    ///
    /// Uncached on palantir's side. The caller's atlas is the cache — this is
    /// the same call palantir's own text backend makes on an atlas miss, and
    /// caching it twice would be paying for a copy nobody reads.
    pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<GlyphImage> {
        self.session.rasterize(glyph)
    }
}

/// One unwrapped line's shape request.
fn request(text: &str, font: GlyphFont) -> TextShapeRequest<'_> {
    TextShapeRequest::unbounded(
        text,
        font.size_px,
        font.line_height_px,
        font.family,
        font.weight,
    )
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

    /// The face and weight [`GlyphFont::new`] writes out are the ones the
    /// enums beside it call default.
    ///
    /// A `const fn` cannot ask a derived [`Default`], so the two are spelled
    /// there — and a `#[default]` moved to the other variant would leave that
    /// silently disagreeing with every other caller of the same enum.
    #[test]
    fn the_stock_font_is_the_default_face_and_weight() {
        // In a const context, which is the whole point of the constructor
        // being one: a caller pinning a font it never varies gets to state it
        // as a value rather than build one per call.
        const STOCK: GlyphFont = GlyphFont::new(16.0);
        assert_eq!(STOCK.family, FontFamily::default());
        assert_eq!(STOCK.weight, FontWeight::default());
        // Led at its own size, which is what "no stack to sit in" comes to.
        assert_eq!(STOCK.line_height_px, 16.0);
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
        assert_eq!(glyphs.measure("", font, 1.0), Size::ZERO);
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

        let at_one = glyphs.measure("abc", font, 1.0);
        let at_two = glyphs.measure("abc", font, 2.0);
        assert!((at_two.w - at_one.w * 2.0).abs() < 1e-3);
        assert!(at_one.w > 0.0 && at_one.h > 0.0);
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
