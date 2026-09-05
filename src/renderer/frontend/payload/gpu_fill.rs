//! The colour lanes every GPU fill writes.

use crate::primitives::color::RgbaF16;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;

/// The three lanes a fill is, whatever tier draws it.
///
/// One type and one set of names for one fact, so a reader who knows a
/// quad's fill knows a curve's. Both draw payloads embed it, and
/// [`BrushSource::gpu_fill`](super::brush_source::BrushSource::gpu_fill)
/// is the only place a brush becomes one.
///
/// The gradient *geometry* lane is not here: a quad carries it and a
/// curve has no room for one, and on a quad it is a reused lane a shadow
/// fills with its own σ and spread rather than an axis — see
/// [`DrawQuadPayload::fill_axis`](super::draw_quad_payload::DrawQuadPayload::fill_axis).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuFill {
    /// Linear-RGB, straight alpha. Zeroed for a gradient, where the
    /// atlas row at [`Self::lut_row`] supplies the colour instead.
    pub(crate) color: RgbaF16,
    /// Low byte is the kind tag; bits 8..16 carry `Spread` for the
    /// gradient variants.
    pub(crate) kind: FillKind,
    /// Gradient atlas row when [`Self::kind`] is a gradient, else
    /// [`LutRow::FALLBACK`].
    pub(crate) lut_row: LutRow,
}

impl GpuFill {
    /// This fill with its opacity scaled by `by`.
    ///
    /// One lane covers both kinds, and that is the point of the layout: a
    /// solid's colour lane holds its real alpha, and a gradient's holds
    /// an opacity multiplier that starts at one because the atlas row
    /// supplies the colour. Scaling the alpha lane is therefore the whole
    /// operation either way. See
    /// [`BrushSource::gpu_fill`](crate::renderer::frontend::payload::brush_source::BrushSource::gpu_fill).
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        Self {
            color: self.color.faded(by),
            ..self
        }
    }

    /// Whether this fill paints nothing.
    ///
    /// A gradient always paints: its colour lane is zeroed by
    /// construction and the atlas row carries the real stops, whose
    /// all-transparent case `Brush::is_noop` catches before lowering. So
    /// the colour only decides for a kind that reads it.
    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        !self.kind.is_gradient() && self.color.is_noop()
    }
}
