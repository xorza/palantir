//! Palantir-native vocabulary for the shaper's render side, so cosmic and swash types never
//! cross out of `src/text/`.

use crate::primitives::geometry::urect::URect;
use cosmic_text::{CacheKey, SubpixelBin};
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunPlacement {
    /// Top-left of the run's box; its fractional part feeds subpixel binning.
    pub(crate) origin: Vec2,
    pub(crate) scale: f32,
    /// Whole-line y-cull bounds, `None` when the caller clips with its own scissor.
    pub(crate) bounds: Option<URect>,
}

/// One glyph's physical-px placement plus its opaque raster key; `x`/`y` position the image before
/// its raster bearing ([`RasterImage::bearing`](crate::widget::RasterImage)). Public so callers drawing
/// their own text get the backend's answer; see [`TextGlyphs`](crate::widget::TextGlyphs).
#[derive(Clone, Copy, Debug)]
pub struct PlacedGlyph {
    /// Raster identity of the glyph.
    pub raster_key: GlyphRasterKey,
    /// X origin in px.
    pub x: i32,
    /// Y origin in px.
    pub y: i32,
}

/// Opaque per-glyph rasterization identity: cosmic's `CacheKey` behind a newtype so no cosmic-text
/// type reaches the public surface; atlases can still hash and compare it.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct GlyphRasterKey(pub(crate) CacheKey);

/// A physical-px origin: integer part plus cosmic's packed subpixel bins.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubpixelOrigin {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Bits 0-1: `y_bin`; bits 2-3: `x_bin`.
    pub(crate) bins: u8,
}

impl SubpixelOrigin {
    /// Splits `origin` with the binning `LayoutGlyph::physical` folds into each raster key, so the
    /// encoded-run identity cannot drift from cosmic's.
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
