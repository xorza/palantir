//! Triangle-mesh builder; lowers to `ShapeRecord::Mesh`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::PaintedBy;
use crate::shape::draw::Draw;
use crate::shape::geometry::point_placement::PointPlacement;
use crate::shape::sealed;

/// A colored triangle mesh, its vertices logical px from its rect's top-left: the owner's
/// arranged rect or [`Self::at`] a rect of its own, its vertices fit into it by [`Self::fit`]. A
/// tint that multiplies its colors is its paint ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Debug)]
#[must_use]
pub struct MeshShape<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) placement: PointPlacement,
}

impl<'a> MeshShape<'a> {
    pub(super) const fn new(mesh: &'a Mesh) -> Self {
        Self {
            mesh,
            placement: PointPlacement::NATURAL,
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::is_offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.placement.local_rect = Some(rect);
        self
    }

    /// Fit the vertices into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement.fit = fit;
        self
    }
}

impl PaintedBy<RgbaF32> for MeshShape<'_> {}

/// A mesh whose vertex colors a tint multiplies.
impl sealed::DrawWith<RgbaF32> for MeshShape<'_> {
    type Prepared = RgbaF32;

    fn prepare(paint: RgbaF32) -> RgbaF32 {
        paint
    }

    /// # Panics
    ///
    /// Unless the tint is a [color](crate::widget::domain::is_color).
    #[track_caller]
    fn check(&self, tint: &RgbaF32) {
        let _ = domain::color(*tint);
    }

    fn draw<'a>(&'a self, tint: &'a RgbaF32) -> Draw<'a> {
        Draw::Mesh {
            shape: self,
            tint: *tint,
        }
    }
}
