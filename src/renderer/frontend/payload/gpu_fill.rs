//! The colour lanes every GPU fill writes.

use crate::primitives::color::RgbaF16;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;

/// The three lanes a fill is, whatever tier draws it.
///
/// One type and one set of names for one fact, so a reader who knows a
/// quad's fill knows a curve's. Both draw payloads embed it, and
/// [`BrushSource::gpu_fill`](super::brush_source::BrushSource::gpu_fill)
/// is the only place a brush becomes one, and [`Self::curve`] the only
/// place a curve's stroke does.
///
/// The gradient *geometry* lane is not here: a quad carries it and a
/// curve has no room for one, and on a quad it is a reused lane a shadow
/// fills with its own σ and spread rather than an axis — see
/// [`DrawQuadPayload::fill_axis`](super::draw_quad_payload::DrawQuadPayload::fill_axis).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuFill {
    /// Linear-RGB, straight alpha. For a solid it is the colour; for a
    /// gradient or a ramp it multiplies, channel by channel, the colour
    /// sampled from the atlas row at [`Self::lut_row`]. Either way, the
    /// paint is invisible exactly when this alpha is zero.
    pub(crate) color: RgbaF16,
    /// Low byte is the kind tag; bits 8..16 carry `Spread` for the
    /// gradient variants.
    pub(crate) kind: FillKind,
    /// Atlas row when [`Self::kind`] is a gradient or a ramp, else
    /// [`LutRow::FALLBACK`].
    pub(crate) lut_row: LutRow,
}

impl GpuFill {
    /// A curve's fill: its stroke colour alone, or multiplying the ramp
    /// baked at `ramp_row`. The one place a curve's fill is made, so a
    /// curve can carry no fill kind but these two.
    #[inline]
    pub(crate) fn curve(color: RgbaF16, ramp_row: Option<LutRow>) -> Self {
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

    /// This fill with its opacity scaled by `by`.
    ///
    /// One lane covers every kind, and that is the point of the layout:
    /// the colour lane's alpha is the paint's alpha, whether it is the
    /// colour or the multiplier on a sample. Scaling it is therefore the
    /// whole operation either way. See
    /// [`BrushSource::gpu_fill`](crate::renderer::frontend::payload::brush_source::BrushSource::gpu_fill).
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// Whether this fill paints nothing: its alpha is zero, which the
    /// shader multiplies into every kind. A ramp whose stops are all
    /// transparent is caught before lowering.
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        self.color.is_noop()
    }
}
