//! What one [`TextGlyphs::line`](crate::widget::TextGlyphs::line) call lays out.

use crate::text::glyph_font::GlyphFont;

/// Text laid out without wrapping, in `font`, for rasterizing at `raster_scale`.
#[derive(Clone, Copy, Debug)]
pub struct GlyphLine<'a> {
    /// The characters to lay out; a `\n` still breaks lines.
    pub text: &'a str,
    /// The face and metrics the text is shaped in.
    pub font: GlyphFont,
    /// Logical→target scale the glyphs are rasterized at, as
    /// [`GpuFrameContext::raster_scale`](crate::GpuFrameContext::raster_scale).
    pub raster_scale: f32,
}
