//! Palantir-native vocabulary for the render side of the shaper: the wgpu
//! text backend drives the measurer through these glyph placements
//! ([`PlacedGlyph`]) and bitmaps ([`GlyphImage`]), so cosmic types
//! (`Buffer`, `FontSystem`, `SwashCache`) never cross out of `src/text/`.
//! [`TextGlyphs`](crate::TextGlyphs) is the lease it drives them through.

use crate::primitives::urect::URect;
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use cosmic_text::{CacheKey, SubpixelBin};
use glam::Vec2;

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
///
/// Public because a caller drawing its own text needs the same answer the
/// text backend does — see [`TextGlyphs`](crate::TextGlyphs).
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub raster_key: GlyphRasterKey,
    pub x: i32,
    pub y: i32,
}

/// One rasterized glyph bitmap.
#[derive(Debug)]
pub struct GlyphImage {
    pub kind: ContentType,
    pub placement: GlyphPlacement,
    /// Tightly packed rows, `width × height` pixels; 1 byte/px for
    /// [`ContentType::Mask`], 4 (RGBA) for [`ContentType::Color`].
    pub data: Vec<u8>,
}

/// Bitmap extents plus bearing of a rasterized glyph, relative to its
/// [`PlacedGlyph`] position.
#[derive(Clone, Copy, Debug)]
pub struct GlyphPlacement {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

/// Opaque per-glyph rasterization identity: cosmic's `CacheKey` (font,
/// glyph id, scaled size, subpixel bins, flags) behind a newtype so the
/// renderer's atlas can key on it without seeing cosmic types.
/// Minted during the glyph walk in `text::cosmic` and consumed by the
/// renderer's atlas.
///
/// Opaque to the outside as well as to the renderer: the field stays
/// crate-private, so an external atlas can hash and compare one without
/// cosmic-text appearing anywhere in palantir's public surface.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct GlyphRasterKey(pub(crate) CacheKey);

/// A physical-px origin split into its integer part plus cosmic's packed
/// 4-bin subpixel remainder.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubpixelOrigin {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Bits 0-1: `y_bin`; bits 2-3: `x_bin` (cosmic's four subpixel
    /// bins, 2 bits each).
    pub(crate) bins: u8,
}

impl SubpixelOrigin {
    /// Split `origin` — the exact binning `LayoutGlyph::physical` folds
    /// into each glyph's raster key, so the renderer's encoded-run
    /// identity can't drift from cosmic's.
    pub(crate) fn of(origin: Vec2) -> Self {
        let (x, x_bin) = SubpixelBin::new(origin.x);
        let (y, y_bin) = SubpixelBin::new(origin.y);
        Self {
            x,
            y,
            bins: ((x_bin as u8) << 2) | (y_bin as u8),
        }
    }
}
