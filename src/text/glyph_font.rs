//! The face a shaping call asks for: font and size parameters, named once so they travel together.

use crate::primitives::math::domain;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;

/// Which face to shape in, and how big. Sizes are logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct GlyphFont {
    /// Font size in logical px.
    pub font_size: f32,
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
    /// Whether `(font_size, leading)` names a face the shaper accepts, each a
    /// [font length](domain::is_font_length); scalars, so the theme can check a derived line
    /// height before a face exists.
    pub(crate) const fn metrics_are_valid(font_size: f32, line_height: f32) -> bool {
        domain::is_font_length(font_size) && domain::is_font_length(line_height)
    }

    pub(crate) const fn metrics_valid(&self) -> bool {
        Self::metrics_are_valid(self.font_size, self.line_height)
    }

    /// `font_size` in the default family, weight and style, led at its own size.
    pub const fn new(font_size: f32) -> Self {
        Self {
            font_size,
            line_height: font_size,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }
    }

    /// This face led at `line_height` logical px.
    pub const fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// This face in `family`.
    pub const fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    /// This face at `weight`, on the CSS 1–1000 scale.
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// This face in `slant`.
    pub const fn with_slant(mut self, slant: FontSlant) -> Self {
        self.slant = slant;
        self
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

    /// Each `with_*` sets its field alone: every field of the result but the named one is the
    /// stock face's.
    #[test]
    fn each_with_setter_sets_its_field_alone() {
        const STOCK: GlyphFont = GlyphFont::new(16.0);
        assert_eq!(
            STOCK.with_line_height(20.0),
            GlyphFont {
                line_height: 20.0,
                ..STOCK
            }
        );
        assert_eq!(
            STOCK.with_family(FontFamily::MONO),
            GlyphFont {
                family: FontFamily::MONO,
                ..STOCK
            }
        );
        assert_eq!(
            STOCK.with_weight(FontWeight::BOLD),
            GlyphFont {
                weight: FontWeight::BOLD,
                ..STOCK
            }
        );
        assert_eq!(
            STOCK.with_slant(FontSlant::Italic),
            GlyphFont {
                slant: FontSlant::Italic,
                ..STOCK
            }
        );
    }
}
