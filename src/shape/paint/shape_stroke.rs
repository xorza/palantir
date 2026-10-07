//! A lowered shape's stroke.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use std::hash::Hasher;

/// Lowered stroke: a straight-alpha colour and a logical-px width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ShapeStroke {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
}

impl ShapeStroke {
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// The stroke a no-op normalizes to, read as "no stroke here" in a payload.
    pub(crate) const NONE: Self = Self {
        width: 0.0,
        color: RgbaF16::TRANSPARENT,
    };

    #[inline]
    pub(crate) fn hash_into<H: Hasher>(self, h: &mut H) {
        h.write_u64(self.color.as_u64());
        h.write_u32(self.width.to_bits());
    }

    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        is_invisible(self.width) || self.color.is_noop()
    }

    /// Collapses a no-op stroke to [`Self::NONE`], so `DrawQuadPayload::is_noop` can test the stroke by colour alone.
    /// A NaN width normalizes away like any non-painting width; catching it loudly is `Shapes::add`'s job.
    #[inline]
    pub(crate) const fn normalized(self) -> Self {
        if self.is_noop() { Self::NONE } else { self }
    }
}

impl From<&Stroke> for ShapeStroke {
    /// Normalized on the way in: a record's raw bytes are a hash key, and `-0.0`, a sub-`EPS` hair or invisible ink
    /// would each hash apart while painting nothing.
    #[inline]
    fn from(stroke: &Stroke) -> Self {
        Self {
            width: stroke.width,
            color: RgbaF16::from(stroke.color),
        }
        .normalized()
    }
}

impl From<Stroke> for ShapeStroke {
    #[inline]
    fn from(stroke: Stroke) -> Self {
        Self::from(&stroke)
    }
}

impl NanCheck for ShapeStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::paint::shape_stroke::ShapeStroke;

    /// Every stroke that paints nothing (`-0.0` width, a hair below `EPS`, transparent ink) lowers to one value.
    #[test]
    fn every_invisible_stroke_lowers_to_one_value() {
        let cases = [
            Stroke::new(RgbaF32::WHITE, -0.0),
            Stroke::new(RgbaF32::WHITE, 0.0),
            Stroke::new(RgbaF32::WHITE, 1e-9),
            Stroke::new(RgbaF32::TRANSPARENT, 4.0),
        ];
        for stroke in cases {
            assert_eq!(
                ShapeStroke::from(&stroke),
                ShapeStroke::NONE,
                "{stroke:?} paints nothing",
            );
        }
        let visible = Stroke::new(RgbaF32::WHITE, 2.0);
        assert_eq!(
            ShapeStroke::from(&visible),
            ShapeStroke {
                width: 2.0,
                color: RgbaF32::WHITE.into(),
            },
        );
    }
}
