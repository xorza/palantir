//! One native GPU stroke — a cubic or an arc.

use crate::primitives::approx::noop_f32;
use crate::primitives::color::ColorF16;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::scene::shapes::paint::CurveBasis;
use crate::shape::style::LineCap;

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
    pub(crate) origin: glam::Vec2,
    /// Solid stroke colour. Zeroed when `fill_kind` is a gradient —
    /// the LUT row at `fill_lut_row` supplies the colour in that case.
    pub(crate) color: ColorF16,
    pub(crate) width: f32,
    /// Typed Pod wire form; composer widens it only at the GPU
    /// `CurveInstance.cap` boundary.
    pub(crate) cap: LineCap,
    /// Brush kind tag (low byte: 0 = solid, 1 = linear). Only solid +
    /// linear are valid on curves; the lowering hard-asserts.
    pub(crate) fill_kind: FillKind,
    /// Gradient atlas row when `fill_kind` is a gradient, else
    /// [`LutRow::FALLBACK`].
    pub(crate) fill_lut_row: LutRow,
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
        self.fill_kind == FillKind::SOLID && self.color.is_noop()
    }
}
