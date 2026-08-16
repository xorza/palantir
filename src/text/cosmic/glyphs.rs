//! The render side of the measurer: resolving a shaped run to placed
//! glyphs, and rasterizing one glyph.
//!
//! Both answer in palantir-native terms ([`PlacedGlyph`], [`GlyphImage`])
//! so cosmic types stay inside this module tree — `TextShaper`'s render
//! session is the only way in, and [`crate::text::render`] is the
//! vocabulary it speaks.

use crate::primitives::num::F32Ext;
use crate::text::cosmic::{CosmicMeasure, ShapedRun};
use crate::text::render::{
    GlyphImage, GlyphImageKind, GlyphPlacement, GlyphRasterKey, PlacedGlyph, RunPlacement,
};
use crate::text::request::TextShapeRequest;
use cosmic_text::SwashContent;

impl CosmicMeasure {
    /// Resolve `request` to palantir-native glyph placements for the
    /// renderer. Restores the shaped buffer if evicted (truncated runs
    /// restore their unbounded probe internally), walks its layout runs,
    /// y-culls whole lines against `placement.bounds`, and rewrites
    /// `out` with one [`PlacedGlyph`] per surviving glyph. Returns
    /// whether any line was culled — such partial extractions must not
    /// become renderer cache templates (its encoded key carries no
    /// bounds).
    pub(crate) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        let ShapedRun { buffer, left } = self.ensure_buffer(request);

        out.clear();
        let RunPlacement {
            origin,
            scale,
            bounds,
        } = placement;
        // `origin` positions the *measured block*, whose left edge is
        // `left` in buffer space — so pull the origin back by it and the
        // per-glyph offsets land where the measurement said they would.
        // Folding it into the origin rather than into each `physical.x`
        // keeps the subpixel binning consistent with the shift.
        let origin_x = origin.x - left * scale;
        let bounds_top = bounds.min.y as f32;
        let bounds_bot = bounds.max().y as f32;
        let mut culled = false;
        for run in buffer.layout_runs() {
            if (run.line_top + run.line_height) * scale + origin.y < bounds_top {
                culled = true;
                continue;
            }
            if run.line_top * scale + origin.y > bounds_bot {
                culled = true;
                break;
            }
            let line_y_px = (run.line_y * scale).fast_round() as i32;
            for glyph in run.glyphs.iter() {
                // The renderer caches encoded runs on one uniform area
                // colour — correct only while cosmic never produces a
                // per-glyph override ([`attrs_for`] sets no per-span
                // colour). If this fires, per-span colour was added
                // without folding a colour fingerprint into the
                // renderer's `EncodedKey`.
                debug_assert!(
                    glyph.color_opt.is_none(),
                    "per-glyph colour override requires folding colour into EncodedKey",
                );
                let physical = glyph.physical((origin_x, origin.y), scale);
                out.push(PlacedGlyph {
                    raster_key: GlyphRasterKey(physical.cache_key),
                    x: physical.x,
                    y: line_y_px + physical.y,
                });
            }
        }
        culled
    }

    /// Rasterize one glyph via swash, uncached on the cosmic side — the
    /// renderer's atlas is the real cache. `None` when swash cannot
    /// produce an image for the key (e.g. a glyph the face lacks).
    pub(crate) fn rasterize_glyph(&mut self, key: GlyphRasterKey) -> Option<GlyphImage> {
        let image = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, key.0)?;
        let kind = match image.content {
            SwashContent::Color => GlyphImageKind::Color,
            SwashContent::Mask | SwashContent::SubpixelMask => GlyphImageKind::Mask,
        };
        Some(GlyphImage {
            kind,
            placement: GlyphPlacement {
                left: image.placement.left,
                top: image.placement.top,
                width: image.placement.width,
                height: image.placement.height,
            },
            data: image.data,
        })
    }
}
