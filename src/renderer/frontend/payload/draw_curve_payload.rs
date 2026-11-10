//! One native GPU stroke — a cubic or an arc.

use crate::primitives::math::domain::is_invisible;
use crate::primitives::paint::line_style::LineCap;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::curve_basis::CurveBasis;
use glam::Vec2;

/// Native GPU stroke payload, a cubic or an arc per [`CurveBasis`], owner-local. The composer turns
/// it by `spin`, culls it by its turned centerline's bounds grown by the stroke, cap and AA, adds
/// `origin` and the push-transform stack, scales to physical px and pushes `CurveInstance`(s) onto
/// `RenderBuffer.curves`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawCurvePayload {
    pub(crate) basis: CurveBasis,
    pub(crate) origin: Vec2,
    /// A paint animation's turn. The composer turns the basis about the pivot exactly: a Bézier by
    /// affine invariance, a circle by moving its center and shifting both angles.
    pub(crate) spin: Option<Spin>,
    /// Solid or ramp; [`GpuFill::curve`] makes no other kind. A curve reads no gradient geometry lane, so [`GpuFill`] is its whole paint.
    pub(crate) fill: GpuFill,
    pub(crate) width: f32,
    /// Typed Pod wire form, widened at the GPU `CurveInstance.cap` boundary.
    pub(crate) cap: LineCap,
}

impl PaintPayload for DrawCurvePayload {
    /// This draw with alpha scaled by `by`, for [`PaintSink`]'s gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            ..self
        }
    }

    /// Paints nothing for non-positive stroke width, a degenerate arc radius, or a transparent stroke color. An all-transparent ramp is caught earlier by `CurveShape`'s no-op test.
    #[inline]
    fn is_noop(&self) -> bool {
        if is_invisible(self.width) {
            return true;
        }
        if let CurveBasis::Arc(arc) = self.basis
            && is_invisible(arc.radius)
        {
            return true;
        }
        self.fill.is_noop()
    }

    fn spin_mut(&mut self) -> &mut Option<Spin> {
        &mut self.spin
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.curve(self);
    }
}
