//! A lowered brush and the GPU fill lanes it expands into.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::color::ColorF16;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;

/// Lowered brush input. `Solid` carries an 8-byte `ColorF16`;
/// `Gradient` carries the 16-byte atlas row + axis + kind resolved for
/// this encode pass.
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrushSource {
    Solid(ColorF16),
    Gradient(ResolvedGradient),
}

impl BrushSource {
    /// Lower to the GPU fill fields shared by every draw-rect/curve
    /// payload: a `Solid` carries its colour with the `SOLID` kind and
    /// the magenta fallback row; a `Gradient` zeroes the colour (the
    /// atlas row supplies it) and forwards kind/row/axis.
    #[inline]
    pub(crate) fn to_gpu_fields(self) -> GpuFillFields {
        match self {
            Self::Solid(c) => GpuFillFields {
                color: c,
                kind: FillKind::SOLID,
                lut_row: LutRow::FALLBACK,
                axis: FillAxis::ZERO,
            },
            Self::Gradient(g) => GpuFillFields {
                color: ColorF16::TRANSPARENT,
                kind: g.kind,
                lut_row: g.row,
                axis: g.axis,
            },
        }
    }
}

/// GPU fill fields a [`BrushSource`] lowers to. Curve payloads carry no
/// `axis`, so they read only the first three.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuFillFields {
    pub(crate) color: ColorF16,
    pub(crate) kind: FillKind,
    pub(crate) lut_row: LutRow,
    pub(crate) axis: FillAxis,
}
