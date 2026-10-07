//! One native GPU stroke — a cubic or an arc.

use crate::primitives::math::domain::is_invisible;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::style::LineCap;
use glam::Vec2;

/// Native GPU stroke payload, a cubic or an arc per [`CurveBasis`]. The composer adds `origin` and the push-transform stack, scales to physical px and pushes `CurveInstance`(s) onto `RenderBuffer.curves`. `bounds` is the owner-local centerline AABB, or the spin and pivot; the composer adds the stroke/cap/AA bound for culling.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawCurvePayload {
    pub(crate) basis: CurveBasis,
    /// Cull bound plus the spin, if any. The composer rotates about the pivot exactly: a Bézier by affine invariance, a circle by moving its centre and shifting both angles.
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: Vec2,
    /// Solid or ramp; [`GpuFill::curve`] makes no other kind. A curve reads no gradient geometry lane, so [`GpuFill`] is its whole paint.
    pub(crate) fill: GpuFill,
    pub(crate) width: f32,
    /// Typed Pod wire form, widened at the GPU `CurveInstance.cap` boundary.
    pub(crate) cap: LineCap,
}

impl DrawCurvePayload {
    /// This draw with alpha scaled by `by`, for [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink)'s gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            fill: self.fill.faded(by),
            ..self
        }
    }

    /// Paints nothing for non-positive stroke width, a degenerate arc radius, or a transparent stroke colour. An all-transparent ramp is caught earlier by `CurveShape`'s no-op test.
    #[inline]
    pub(crate) const fn is_noop(&self) -> bool {
        if is_invisible(self.width) {
            return true;
        }
        if let CurveBasis::Arc { radius, .. } = self.basis
            && is_invisible(radius)
        {
            return true;
        }
        self.fill.is_noop()
    }
}
