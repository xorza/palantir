//! Composited icon draw records consumed by the icon backend.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::primitives::color::ColorU8;
use glam::IVec2;

/// One icon draw, placed in physical-pixel space.
///
/// Deferred the way a [`TextDrawRow`](crate::renderer::render_buffer::text::TextDrawRow)
/// is: the composer resolves *where* and *how big*, and the backend resolves
/// *what pixels* — rasterizing on an atlas miss and emitting the quad. That
/// split is what lets an icon be rasterized at its true device size, which is
/// only known once the display scale and every ancestor transform have been
/// applied.
///
/// No `bounds` field, unlike a text row: an icon is one quad rather than a run
/// of lines, so the group's own scissor is the whole of its clipping and there
/// is nothing to pre-cull line by line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IconDrawRow {
    /// Which icon at which physical size — already through the raster-size
    /// ladder, so this is exactly the raster the atlas will hold.
    pub(crate) key: IconRasterKey,
    /// Top-left of the quad in physical px. Whole pixels: the atlas sampler is
    /// `Nearest` and the quad is drawn at the raster's own dimensions, so a
    /// fractional origin would blur what the rasterizer got exactly right.
    pub(crate) origin: IVec2,
    /// Straight-alpha **linear** RGBA, like a text run's colour. Multiplies a
    /// mask icon whole; a colour icon takes the alpha alone.
    pub(crate) color: ColorU8,
    /// Draw a colour icon as its own luminance — the backend folds this into
    /// the quad's packed uv field rather than spending an instance lane on it.
    pub(crate) desaturate: bool,
}
