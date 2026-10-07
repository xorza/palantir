//! A lowered brush and the GPU fill lanes it expands into.

use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;

/// Lowered brush input: `Solid` (an `RgbaF16`) or `Gradient` (atlas row, axis, kind).
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrushSource {
    Solid(RgbaF16),
    Gradient(ResolvedGradient),
}

impl BrushSource {
    /// Lower to the colour lanes every draw payload carries. A `Gradient`'s colour lane starts white, the identity of the multiply [`GpuFill::color`] applies, so [`DrawQuadPayload::faded`] fades it with no second uniform.
    ///
    /// [`DrawQuadPayload::faded`]: crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload::faded
    #[inline]
    pub(crate) const fn gpu_fill(self) -> GpuFill {
        match self {
            Self::Solid(color) => GpuFill {
                color,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
            },
            Self::Gradient(g) => GpuFill {
                color: RgbaF16::WHITE,
                kind: g.kind,
                lut_row: g.lut_row,
            },
        }
    }

    /// The gradient geometry a quad reads; zeroed for a solid so Pod-byte cache keys are deterministic.
    #[inline]
    pub(crate) const fn fill_axis(self) -> FillAxis {
        match self {
            Self::Solid(_) => FillAxis::ZERO,
            Self::Gradient(g) => g.axis,
        }
    }
}
