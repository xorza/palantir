//! [`LoweredStroke`]: a curve's or an arc's stroke, lowered.

use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::stroke::Stroke;
use std::hash::Hasher;

/// A lowered stroke: a straight-alpha color and a logical-px width, centered on the path. Its cap
/// and ramp ride the record beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LoweredStroke {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
}

impl LoweredStroke {
    /// `stroke`'s width and color, the color quantized to f16. A named method, not `From`, as it
    /// loses the cap, the join, the ramp and precision; the lowering dropped a stroke that paints
    /// nothing.
    #[inline]
    pub(crate) fn lower(stroke: Stroke) -> Self {
        Self {
            width: stroke.width,
            color: stroke.color.to_f16(),
        }
    }

    #[inline]
    pub(crate) fn hash_into<H: Hasher>(self, h: &mut H) {
        h.write_u64(self.color.as_u64());
        h.write_u32(self.width.to_bits());
    }
}

impl NanCheck for LoweredStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}
