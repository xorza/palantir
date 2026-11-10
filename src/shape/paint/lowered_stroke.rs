//! [`LoweredStroke`]: a stroke, lowered, the paint of every stroke record.

use crate::common::hash::Hasher;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::curve_ramp::CurveRamp;
use std::hash::{Hash, Hasher as _};

/// A lowered [`Stroke`]: a straight-alpha color and a logical-px width, centered on the path, its
/// cap and join, and the ramp its color multiplies, interned. A polyline's ramp is
/// [`CurveRamp::None`], as a polyline draws none, and a curve's join turns no joint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredStroke {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    pub(crate) ramp: CurveRamp,
}

impl LoweredStroke {
    /// `stroke` with its ramp interned as `ramp`, the color quantized to f16. A named method, not
    /// `From`, as it loses precision; the lowering dropped a stroke that paints nothing.
    #[inline]
    pub(crate) fn lower(stroke: Stroke, ramp: CurveRamp) -> Self {
        Self {
            width: stroke.width,
            color: stroke.color.to_f16(),
            cap: stroke.cap,
            join: stroke.join,
            ramp,
        }
    }

    /// Feed the stroke into a record's hash: its width and color, its cap and join, and its ramp
    /// by content hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        h.write_u64(self.color.as_u64());
        h.write_u32(self.width.to_bits());
        h.write_u8(self.cap as u8);
        h.write_u8(self.join as u8);
        match self.ramp {
            CurveRamp::None => h.write_u8(0),
            CurveRamp::Interned { id: _, hash } => {
                h.write_u8(1);
                hash.hash(h);
            }
        }
    }
}

impl NanCheck for LoweredStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan()
    }
}
