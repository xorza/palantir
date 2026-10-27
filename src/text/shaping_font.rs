//! [`ShapingFont`]: a face as one shaper keys it.

use crate::primitives::math::nan::NanCheck;
use crate::text::family_keys::{FamilyKey, FamilyKeys};
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;

/// A [`GlyphFont`] whose family one shaper's [`FamilyKeys`] keyed: what a recorded text shape and
/// a cache key carry, so neither holds a family's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ShapingFont {
    pub(crate) size: f32,
    pub(crate) line_height: f32,
    pub(crate) family: FamilyKey,
    pub(crate) weight: FontWeight,
    pub(crate) slant: FontSlant,
}

impl ShapingFont {
    /// `font` with its family keyed in `keys`.
    pub(crate) fn new(font: GlyphFont, keys: &FamilyKeys) -> Self {
        let GlyphFont {
            size,
            line_height,
            family,
            weight,
            slant,
        } = font;
        Self {
            size,
            line_height,
            family: keys.key(family),
            weight,
            slant,
        }
    }

    pub(crate) const fn metrics_valid(&self) -> bool {
        GlyphFont::metrics_are_valid(self.size, self.line_height)
    }
}

impl NanCheck for ShapingFont {
    /// Only the two metrics can be NaN; the three face axes are integral.
    fn has_nan(&self) -> bool {
        self.size.is_nan() || self.line_height.is_nan()
    }
}
