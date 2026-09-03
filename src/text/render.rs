//! Palantir-native vocabulary for the render side of the shaper: the wgpu
//! text backend drives the measurer through these glyph placements
//! ([`PlacedGlyph`]) and the bitmaps they resolve to
//! ([`RasterImage`](crate::RasterImage)), so cosmic and swash types
//! (`Buffer`, `FontSystem`, `ScaleContext`) never cross out of
//! `src/text/`. [`TextGlyphs`](crate::TextGlyphs) is the lease it drives
//! them through.

use crate::primitives::urect::URect;
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
    /// Whole-line y-cull bounds, or `None` for a run with nothing to
    /// cull against — a caller drawing into its own target, which clips
    /// with its own scissor. The GPU scissor is the real pixel clip
    /// either way.
    pub(crate) bounds: Option<URect>,
}

/// One glyph resolved to physical-px placement plus its opaque raster
/// key. `x`/`y` position the glyph image before its raster bearing
/// ([`RasterImage::bearing`](crate::RasterImage)) is applied.
///
/// Public because a caller drawing its own text needs the same answer the
/// text backend does — see [`TextGlyphs`](crate::TextGlyphs).
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    pub raster_key: GlyphRasterKey,
    pub x: i32,
    pub y: i32,
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
