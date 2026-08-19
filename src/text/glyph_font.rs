//! The face a shaping call is asked for: the four parameters that pick a
//! font and a size, named once so they travel together.
//!
//! Shared vocabulary rather than one caller's parameter bundle. The
//! authoring shape, the record it lowers to, the probe input, layout's
//! own lowering, the shape-cache key, and the public
//! [`TextGlyphs`](crate::TextGlyphs) lease all state a face as this one
//! value — so the five that mirror each other mirror in one field, and a
//! swapped pair of metrics is a type error rather than a silent mis-key
//! that only shows up as a cache miss.

use crate::primitives::nan::NanCheck;
use crate::text::{FontFamily, FontWeight};

/// Which face to shape in, and how big.
///
/// Sizes are logical pixels; the raster scale is
/// [`TextGlyphs::line`](crate::TextGlyphs::line)'s, because
/// it is a property of the surface being drawn into rather than of the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphFont {
    pub size_px: f32,
    /// Leading is the caller's to choose. Palantir's own widgets derive one
    /// from the type scale; a single line pinned to a point in space has no
    /// stack to sit in, so this defaults to the size itself.
    pub line_height_px: f32,
    pub family: FontFamily,
    pub weight: FontWeight,
}

impl GlyphFont {
    /// `size_px` in the default family and weight, led at its own size.
    ///
    /// Every field is public, so anything else is a struct update over this —
    /// `GlyphFont { family: FontFamily::Mono, ..GlyphFont::new(16.0) }`, which
    /// holds in a `const` too.
    ///
    /// The two defaults are spelled out rather than asked of [`Default`],
    /// which a derive does not make a `const fn`. They are the `#[default]`
    /// variants of the enums beside them and have to stay that.
    pub const fn new(size_px: f32) -> Self {
        Self {
            size_px,
            line_height_px: size_px,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        }
    }
}

impl NanCheck for GlyphFont {
    /// Only the two metrics can be NaN; the family and weight are enums.
    fn has_nan(&self) -> bool {
        self.size_px.is_nan() || self.line_height_px.is_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::text::glyph_font::GlyphFont;
    use crate::text::{FontFamily, FontWeight};

    /// The face and weight [`GlyphFont::new`] writes out are the ones the
    /// enums beside it call default.
    ///
    /// A `const fn` cannot ask a derived [`Default`], so the two are spelled
    /// there — and a `#[default]` moved to the other variant would leave that
    /// silently disagreeing with every other caller of the same enum.
    #[test]
    fn the_stock_font_is_the_default_face_and_weight() {
        // In a const context, which is the whole point of the constructor
        // being one: a caller pinning a font it never varies gets to state it
        // as a value rather than build one per call.
        const STOCK: GlyphFont = GlyphFont::new(16.0);
        assert_eq!(STOCK.family, FontFamily::default());
        assert_eq!(STOCK.weight, FontWeight::default());
        // Led at its own size, which is what "no stack to sit in" comes to.
        assert_eq!(STOCK.line_height_px, 16.0);
    }
}
