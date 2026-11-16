//! [`LoweredStroke`]: a stroke, lowered, the paint of every stroke record.

use crate::common::hash::Hasher;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::line_style::{Dash, LineCap, LineJoin};
use crate::primitives::paint::stroke::Stroke;
use crate::shape::paint::curve_ramp::CurveRamp;
use std::hash::{Hash, Hasher as _};

/// A lowered [`Stroke`]: a straight-alpha color and a logical-px width, centered on the path, its
/// cap and join, the ramp its color multiplies, interned, and its dash pattern. A polyline's ramp
/// is [`CurveRamp::None`], as a polyline draws none, nor a dash, and a curve's join turns no
/// joint. A pattern that breaks nothing is lowered to `None`, so two strokes that draw one line
/// hash alike.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoweredStroke {
    pub(crate) width: f32,
    pub(crate) color: RgbaF16,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    pub(crate) ramp: CurveRamp,
    pub(crate) dash: Option<Dash>,
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
            dash: stroke.dash.filter(|dash| !dash.is_solid()),
        }
    }

    /// Feed the stroke into a record's hash: its width and color, its cap and join, its ramp by
    /// content hash, and its dash.
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
        match self.dash {
            None => h.write_u8(0),
            Some(Dash { on, off }) => {
                h.write_u8(1);
                h.write_u32(on.to_bits());
                h.write_u32(off.to_bits());
            }
        }
    }
}

impl NanCheck for LoweredStroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.color.has_nan() || self.dash.has_nan()
    }
}

#[cfg(test)]
mod tests {
    use crate::common::hash::Hasher;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::paint::line_style::Dash;
    use crate::primitives::paint::stroke::Stroke;
    use crate::shape::paint::curve_ramp::CurveRamp;
    use crate::shape::paint::lowered_stroke::LoweredStroke;
    use std::hash::Hasher as _;

    /// A pattern that breaks nothing, no gap or a dash and a gap too short to see, lowers to no
    /// dash, so it hashes as the whole line it draws; one with a gap is kept as given.
    #[test]
    fn a_pattern_that_breaks_nothing_lowers_to_no_dash() {
        let line = Stroke::new(RgbaF32::WHITE, 2.0);
        let lower = |stroke| LoweredStroke::lower(stroke, CurveRamp::None);
        let hash = |stroke: LoweredStroke| {
            let mut h = Hasher::new();
            stroke.hash_into(&mut h);
            h.finish()
        };
        for solid in [Dash::new(30.0, 0.0), Dash::new(0.0, 0.0)] {
            let lowered = lower(line.with_dash(solid));
            assert_eq!(lowered.dash, None, "{solid:?}");
            assert_eq!(hash(lowered), hash(lower(line)), "{solid:?}");
        }
        let dashed = Dash::new(6.0, 4.0);
        assert_eq!(lower(line.with_dash(dashed)).dash, Some(dashed));
    }
}
