//! Aperture-native vocabulary for the render side of the shaper: the wgpu
//! text backend drives `CosmicMeasure` through these glyph placements
//! ([`PlacedGlyph`]) and bitmaps ([`GlyphImage`]), so cosmic types
//! (`Buffer`, `FontSystem`, `SwashCache`) never cross out of `src/text/`.
//! `TextShaper::render_session` leases the measurer for the duration of
//! one batch's encoded-cache misses; the backend's all-hit fast path
//! never opens one.

use crate::primitives::urect::URect;
use crate::text::TextShapeRequest;
use crate::text::cosmic::CosmicMeasure;
use cosmic_text::{CacheKey, SubpixelBin};
use glam::Vec2;
use std::cell::RefMut;

/// Exclusive render-side lease on the shared shaper, minted by
/// `TextShaper::render_session`. The backend drives shaping through this
/// and never names [`CosmicMeasure`], which keeps `text::cosmic` private
/// to this module tree; dropping the lease releases the `RefCell` borrow.
#[derive(Debug)]
pub(crate) struct TextRenderSession<'a> {
    cosmic: RefMut<'a, CosmicMeasure>,
}

impl<'a> TextRenderSession<'a> {
    pub(super) fn new(cosmic: RefMut<'a, CosmicMeasure>) -> Self {
        Self { cosmic }
    }

    /// Resolve one run's glyphs, restoring an evicted shaped buffer on the
    /// way. Returns whether any line was y-culled (partial extractions
    /// must not be cached).
    pub(crate) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        self.cosmic.extract_glyphs(request, placement, out)
    }

    /// Rasterize one glyph; `None` when the face cannot produce an image.
    pub(crate) fn rasterize(&mut self, key: GlyphRasterKey) -> Option<GlyphImage> {
        self.cosmic.rasterize_glyph(key)
    }
}

/// Physical-px placement of one text run, input to glyph extraction.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunPlacement {
    /// Top-left of the run's box; the fractional part participates in
    /// cosmic's subpixel binning.
    pub(crate) origin: Vec2,
    /// Full raster scale (frame DPI × per-run transform scale).
    pub(crate) scale: f32,
    /// Whole-line y-cull bounds; the GPU scissor is the real pixel clip.
    pub(crate) bounds: URect,
}

/// One glyph resolved to physical-px placement plus its opaque raster
/// key. `x`/`y` position the glyph image before its raster bearing
/// ([`GlyphPlacement`]'s `left`/`top`) is applied.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacedGlyph {
    pub(crate) raster_key: GlyphRasterKey,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

/// One rasterized glyph bitmap.
#[derive(Debug)]
pub(crate) struct GlyphImage {
    pub(crate) kind: GlyphImageKind,
    pub(crate) placement: GlyphPlacement,
    /// Tightly packed rows, `width × height` pixels; 1 byte/px for
    /// [`GlyphImageKind::Mask`], 4 (RGBA) for [`GlyphImageKind::Color`].
    pub(crate) data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlyphImageKind {
    /// Alpha-only coverage.
    Mask,
    /// Full-colour (emoji) RGBA.
    Color,
}

/// Bitmap extents plus bearing of a rasterized glyph, relative to its
/// [`PlacedGlyph`] position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphPlacement {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Opaque per-glyph rasterization identity: cosmic's `CacheKey` (font,
/// glyph id, scaled size, subpixel bins, flags) behind a newtype so the
/// renderer's atlas can key on it without seeing cosmic types.
/// Minted during the glyph walk in `text::cosmic` and consumed by the
/// renderer's atlas.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct GlyphRasterKey(pub(crate) CacheKey);

/// Split a physical-px origin into its integer part plus cosmic's
/// packed 4-bin subpixel remainder — the exact binning
/// `LayoutGlyph::physical` folds into each glyph's raster key, so the
/// renderer's encoded-run identity can't drift from cosmic's.
pub(crate) fn subpixel_origin(origin: Vec2) -> SubpixelOrigin {
    let (x, x_bin) = SubpixelBin::new(origin.x);
    let (y, y_bin) = SubpixelBin::new(origin.y);
    SubpixelOrigin {
        x,
        y,
        bins: ((x_bin as u8) << 2) | (y_bin as u8),
    }
}

/// [`subpixel_origin`]'s named result.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubpixelOrigin {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Bits 0-1: `y_bin`; bits 2-3: `x_bin` (cosmic's four subpixel
    /// bins, 2 bits each).
    pub(crate) bins: u8,
}

#[cfg(test)]
mod internals {
    use crate::text::render::GlyphRasterKey;
    use cosmic_text::{CacheKey, CacheKeyFlags, SubpixelBin, Weight, fontdb};

    impl GlyphRasterKey {
        /// Distinct dummy keys for the renderer's atlas tests — the
        /// only way to mint one outside a real glyph walk.
        pub(crate) fn for_test(glyph_id: u16) -> Self {
            Self(CacheKey {
                font_id: fontdb::ID::dummy(),
                glyph_id,
                font_size_bits: 14.0_f32.to_bits(),
                x_bin: SubpixelBin::Zero,
                y_bin: SubpixelBin::Zero,
                font_weight: Weight::NORMAL,
                flags: CacheKeyFlags::empty(),
            })
        }
    }
}
