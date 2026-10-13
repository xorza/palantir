//! Composited icon draw records consumed by the icon backend.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use glam::{IVec2, U16Vec2};

/// One icon draw in physical-pixel space. The composer resolves where and how big; the backend resolves pixels (rasterizing on an atlas miss), so the icon rasterizes at its true device size.
///
/// No `bounds` field, unlike a text row: one quad, so the group's scissor is all the clipping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IconDrawRow {
    /// Which icon at which physical size, already through the raster-size ladder.
    pub(crate) key: IconRasterKey,
    /// Quad top-left in physical px. Whole pixels: in the exact band the quad is drawn `Nearest` at raster size, so a fraction would blur.
    pub(crate) origin: IVec2,
    /// Quad extents in physical px: the raster's own in the exact band; above it the icon's whole-pixel box, resampled from a nearby ladder rung (or the capped one far below).
    pub(crate) size: U16Vec2,
    /// Straight-alpha **linear** RGBA. Multiplies a mask icon whole; a colour icon takes the alpha alone.
    pub(crate) color: RgbaF16,
    /// Draw a colour icon as its own luminance; folded into the quad's packed uv field.
    pub(crate) desaturate: bool,
}
