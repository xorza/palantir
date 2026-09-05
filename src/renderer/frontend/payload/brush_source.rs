//! A lowered brush and the GPU fill lanes it expands into.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::RgbaF16;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;

/// Lowered brush input. `Solid` carries an 8-byte `RgbaF16`;
/// `Gradient` carries the 16-byte atlas row + axis + kind resolved for
/// this encode pass.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrushSource {
    Solid(RgbaF16),
    Gradient(ResolvedGradient),
}

impl BrushSource {
    /// Lower to the colour lanes every draw payload carries: a `Solid`
    /// takes its colour with the `SOLID` kind and the magenta fallback
    /// row; a `Gradient` takes the atlas row instead, and its colour lane
    /// becomes an opacity multiplier that starts at one.
    ///
    /// **A gradient's colour lane is unread by the atlas path**, because
    /// the row at `lut_row` supplies the colour. That leaves it free, and
    /// the shader's gradient branch multiplies the sampled alpha by it —
    /// which is what lets [`DrawQuadPayload::faded`] fade a gradient fill
    /// with no second uniform and no per-alpha atlas row.
    ///
    /// [`DrawQuadPayload::faded`]: crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload::faded
    #[inline]
    pub(crate) fn gpu_fill(self) -> GpuFill {
        match self {
            Self::Solid(color) => GpuFill {
                color,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
            },
            Self::Gradient(g) => GpuFill {
                color: RgbaF16::opacity_one(),
                kind: g.kind,
                lut_row: g.lut_row,
            },
        }
    }

    /// The gradient geometry a quad reads. Zero for a solid, which the
    /// shader ignores — but zeroed rather than arbitrary, so a Pod-byte
    /// cache key over a solid quad is deterministic.
    #[inline]
    pub(crate) fn fill_axis(self) -> FillAxis {
        match self {
            Self::Solid(_) => FillAxis::ZERO,
            Self::Gradient(g) => g.axis,
        }
    }
}
