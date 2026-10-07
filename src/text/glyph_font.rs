//! The face a shaping call asks for: font and size parameters, named once so they travel together.

use crate::primitives::math::domain::EPS;
use crate::primitives::math::nan::NanCheck;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Which face to shape in, and how big. Sizes are logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphFont {
    /// Size in logical px.
    pub size: f32,
    /// Leading is the caller's choice; defaults to the size itself.
    pub line_height: f32,
    /// Font family.
    pub family: FontFamily,
    /// Which weight to match, on the CSS 1–1000 scale.
    pub weight: FontWeight,
    /// Font slant.
    pub slant: FontSlant,
}

impl GlyphFont {
    /// Shared rejection message, so a bad size reads the same wherever authored.
    pub(crate) const METRICS_ERROR: &'static str =
        "font size and line height must be finite and above the UI epsilon";

    /// Whether `(size, leading)` names a face the shaper accepts; scalars, so the theme can check a derived line height before a face exists.
    pub(crate) const fn metrics_are_valid(size: f32, line_height: f32) -> bool {
        Self::length_is_valid(size) && Self::length_is_valid(line_height)
    }

    /// The half one metric answers alone.
    pub(crate) const fn length_is_valid(px: f32) -> bool {
        px.is_finite() && px > EPS
    }

    pub(crate) const fn metrics_valid(&self) -> bool {
        Self::metrics_are_valid(self.size, self.line_height)
    }

    /// `size` in the default family, weight and style, led at its own size.
    pub const fn new(size: f32) -> Self {
        Self {
            size,
            line_height: size,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }
    }
}

impl NanCheck for GlyphFont {
    /// Only the two metrics can be NaN; the three face axes are integral.
    fn has_nan(&self) -> bool {
        self.size.is_nan() || self.line_height.is_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::FontFamily;
    use crate::text::font_slant::FontSlant;
    use crate::text::font_weight::FontWeight;
    use crate::text::glyph_font::GlyphFont;

    /// Must match the types' own defaults; `const fn` cannot call a derived [`Default`].
    #[test]
    fn the_stock_font_is_the_default_face_and_weight() {
        const STOCK: GlyphFont = GlyphFont::new(16.0);
        assert_eq!(STOCK.family, FontFamily::default());
        assert_eq!(STOCK.weight, FontWeight::default());
        assert_eq!(STOCK.slant, FontSlant::default());
        assert_eq!(STOCK.line_height, 16.0);
    }
}
