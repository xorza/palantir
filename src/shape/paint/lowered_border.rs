//! [`LoweredBorder`]: a box's or a triangle's border, lowered.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use std::hash::Hasher;

/// A lowered border: a straight-alpha color and a logical-px width, inside the shape's edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LoweredBorder {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
}

impl LoweredBorder {
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// The border a no-op normalizes to, read as "no border here" in a payload.
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

    /// Collapses a no-op border to [`Self::NONE`], so `ClosedFill::is_noop` can test the border by color alone.
    /// A NaN width normalizes away like any non-painting width; the shape setters and `Ui::add_shape` catch it loudly.
    #[inline]
    pub(crate) const fn normalized(self) -> Self {
        if self.is_noop() { Self::NONE } else { self }
    }
}

impl LoweredBorder {
    /// `border` lowered: its color quantized to f16, and normalized on the way in, as a record's
    /// raw bytes are a hash key, and `-0.0`, a sub-`EPS` hair or invisible ink would each hash
    /// apart while painting nothing. A named method, not `From`, as it loses both.
    #[inline]
    pub(crate) fn lower(border: Border) -> Self {
        Self {
            width: border.width,
            color: border.color.to_f16(),
        }
        .normalized()
    }
}

impl NanCheck for LoweredBorder {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::border::Border;
    use crate::primitives::paint::color::RgbaF32;
    use crate::shape::paint::lowered_border::LoweredBorder;

    /// Every border that paints nothing (`-0.0` width, a hair below `EPS`, transparent ink) lowers to one value.
    #[test]
    fn every_invisible_border_lowers_to_one_value() {
        let cases = [
            Border::new(RgbaF32::WHITE, -0.0),
            Border::new(RgbaF32::WHITE, 0.0),
            Border::new(RgbaF32::WHITE, 1e-9),
            Border::new(RgbaF32::TRANSPARENT, 4.0),
        ];
        for border in cases {
            assert_eq!(
                LoweredBorder::lower(border),
                LoweredBorder::NONE,
                "{border:?} paints nothing",
            );
        }
        let visible = Border::new(RgbaF32::WHITE, 2.0);
        assert_eq!(
            LoweredBorder::lower(visible),
            LoweredBorder {
                width: 2.0,
                color: RgbaF32::WHITE.to_f16(),
            },
        );
    }
}
