//! One native GPU stroke — a cubic or an arc.

use crate::primitives::approx::noop_f32;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::scene::shapes::paint::CurveBasis;
use crate::shape::style::LineCap;
use glam::Vec2;

/// Native GPU stroke payload — a cubic or an arc, per [`CurveBasis`].
/// The composer adds `origin` and the active push-transform stack
/// before scaling to physical px and pushing the resulting
/// `CurveInstance`(s) onto `RenderBuffer.curves`. `bbox` is the
/// owner-local centerline AABB; the composer applies the shared
/// stroke/cap/AA bound in physical space for culling and overlap.
/// `rotation` carries the spin angle under the pivot contract in the
/// module doc; the composer rotates about that pivot exactly — a Bézier
/// by affine invariance, a circle by moving its centre and shifting
/// both angles.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct DrawCurvePayload {
    pub(crate) basis: CurveBasis,
    /// Cull bound plus the spin, if any. The composer rotates about the
    /// pivot exactly — a Bézier by affine invariance, a circle by moving
    /// its centre and shifting both angles.
    pub(crate) bounds: StrokeBounds,
    pub(crate) origin: Vec2,
    /// Only solid and linear are valid on a curve; the lowering
    /// hard-asserts. A curve reads no gradient geometry lane, so
    /// [`GpuFill`] is the whole of its brush.
    pub(crate) fill: GpuFill,
    pub(crate) width: f32,
    /// Typed Pod wire form; composer widens it only at the GPU
    /// `CurveInstance.cap` boundary.
    pub(crate) cap: LineCap,
}

impl DrawCurvePayload {
    /// Paints nothing when: zero/negative stroke width, a
    /// degenerate arc radius (nothing to trace), or a solid fill that's
    /// fully transparent. Gradient fills always paint (the
    /// all-transparent-stops case is caught by `Brush::is_noop` before
    /// lowering).
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        if noop_f32(self.width) {
            return true;
        }
        if let CurveBasis::Arc { radius, .. } = self.basis
            && noop_f32(radius)
        {
            return true;
        }
        self.fill.is_noop()
    }
}
