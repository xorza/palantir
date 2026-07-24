//! Aperture-native vocabulary for the render side of the shaper: the wgpu
//! text backend drives `CosmicMeasure` through these glyph placements
//! ([`PlacedGlyph`]) and bitmaps ([`GlyphImage`]), so cosmic types
//! (`Buffer`, `FontSystem`, `SwashCache`) never cross out of `src/text/`.
//! `TextShaper::render_session` leases the measurer for the duration of
//! one batch's encoded-cache misses; the backend's all-hit fast path
//! never opens one.

use crate::primitives::urect::URect;
use crate::text::cosmic::GlyphRasterKey;
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
