//! Laying out and rasterizing glyphs for a caller that draws its own text (e.g. a
//! [`GpuView`](crate::GpuView)): where each glyph sits and what it looks like. The
//! atlas, pipeline and blending are the caller's; the font stack is shared.

use crate::primitives::geometry::size::Size;
use crate::primitives::paint::raster_image::RasterImage;
use crate::text::cosmic::CosmicMeasure;
use crate::text::glyph_font::GlyphFont;
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;
use crate::text::wrap::WrapFloor;
use glam::Vec2;
use std::cell::RefMut;

/// A lease on the shaper for laying out and rasterizing glyphs directly. It holds
/// the shaper's exclusive borrow until dropped, so keep one for a batch and do not
/// measure text through the same [`Ui`](crate::Ui) meanwhile. Minted by
/// [`TextShaper::glyphs`](crate::TextShaper::glyphs). **Palantir's own text backend
/// holds one too**; its `extract_glyphs` is the crate-facing half of
/// [`Self::line`], adding the y-cull and run placement.
#[derive(Debug)]
pub struct TextGlyphs<'a> {
    cosmic: RefMut<'a, CosmicMeasure>,
}

impl<'a> TextGlyphs<'a> {
    pub(super) const fn new(cosmic: RefMut<'a, CosmicMeasure>) -> Self {
        Self { cosmic }
    }

    /// Resolve one run's glyphs at `placement`, restoring an evicted shaped buffer.
    /// Returns whether any line was y-culled; a partial extraction must not become
    /// a renderer cache template.
    pub(crate) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        self.cosmic.extract_glyphs(request, placement, out)
    }

    /// Lay `text` out without wrapping at `scale`, rewriting `out` with a
    /// [`PlacedGlyph`] apiece. A `\n` still breaks lines. Positions are relative to
    /// the line's origin (left edge, top of the line box), so a run laid out once
    /// can be placed every frame. Cosmic bins each glyph's own fractional `x *
    /// scale` into its raster key, so an atlas can hold up to four entries per
    /// glyph per size. Empty text, or a `font` whose size or leading is not a
    /// positive finite number, clears `out` and returns.
    pub fn line(&mut self, text: &str, font: GlyphFont, scale: f32, out: &mut Vec<PlacedGlyph>) {
        // One of the two crate edges for a run with nothing to shape; see
        // `TextShapeRequest`.
        let Some(request) = TextShapeRequest::unbounded(text, font) else {
            out.clear();
            return;
        };
        // Placed at its own origin and culled against nothing: the caller positions
        // and clips.
        let placement = RunPlacement {
            origin: Vec2::ZERO,
            scale,
            bounds: None,
        };
        self.extract_glyphs(request, placement, out);
    }

    /// How far `text` reaches in `font`, in the logical pixels `font` is sized in,
    /// without laying it out. A run's advance is not the span of its bitmaps, so
    /// anchoring needs this. **Takes no raster scale, unlike
    /// [`TextGlyphs::line`]**, which rounds each glyph at the raster size, so the
    /// two disagree by that rounding above scale 1: anchor from this, position
    /// glyphs from `line`. Empty text, or a `font` with no usable size, measures
    /// [`Size::ZERO`].
    pub fn measure(&mut self, text: &str, font: GlyphFont) -> Size {
        TextShapeRequest::unbounded(text, font).map_or(Size::ZERO, |request| {
            self.cosmic.root(request, WrapFloor::Skip).extent.size
        })
    }

    /// The bitmap for one glyph, or `None` where the face cannot produce one.
    /// Uncached here (the caller's atlas is the cache); the pixels borrow the
    /// lease's scratch, so copy them before the next glyph.
    pub fn rasterize(&mut self, glyph: GlyphRasterKey) -> Option<RasterImage<'_>> {
        self.cosmic.rasterize_glyph(glyph)
    }
}

#[cfg(test)]
mod tests;
