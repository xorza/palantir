//! The colour lanes every GPU fill writes.

use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;

/// The three lanes a fill is, whatever tier draws it (both draw payloads embed it). A brush becomes one only in
/// [`BrushSource::gpu_fill`](super::brush_source::BrushSource::gpu_fill)
/// and a curve's stroke only in [`Self::curve`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuFill {
    /// Linear-RGB, straight alpha. A solid's colour, or the multiplier on the colour sampled at [`Self::lut_row`];
    /// either way the paint is invisible exactly when this alpha is zero.
    pub(crate) color: RgbaF16,
    /// Low byte is the kind tag; bits 8..16 carry `Spread` for gradients.
    pub(crate) kind: FillKind,
    /// Atlas row for a gradient or ramp, else [`LutRow::FALLBACK`].
    pub(crate) lut_row: LutRow,
}

impl GpuFill {
    /// A curve's fill: its stroke colour alone, or multiplying the ramp baked at `ramp_row`.
    #[inline]
    pub(crate) const fn curve(color: RgbaF16, ramp_row: Option<LutRow>) -> Self {
        match ramp_row {
            None => Self {
                color,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
            },
            Some(lut_row) => Self {
                color,
                kind: FillKind::RAMP,
                lut_row,
            },
        }
    }

    /// This fill with its opacity scaled by `by`; the colour lane's alpha is the paint's alpha for every kind.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// Whether this fill paints nothing: its alpha is zero (all-transparent ramps are caught before lowering).
    #[inline]
    pub(crate) const fn is_noop(self) -> bool {
        self.color.is_noop()
    }
}
