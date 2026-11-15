//! [`ClosedShape`]: a shape that bounds an area, which a box's paint, a material, an image and an
//! effect each cover the one way, so each paint draws over every such shape through one impl.

use crate::primitives::effect::effect_kind::ErasedRead;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint::Paint;
use crate::shape::draw::{ClosedPaintRef, Draw};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::sealed::DrawWith;

/// A rect, a circle or a triangle: the geometry its record holds, and what of a paint it cannot
/// draw.
pub(crate) trait ClosedShape {
    /// The geometry this shape records.
    fn closed_geometry(&self) -> ClosedGeometry;

    /// Panic unless this shape draws `paint`, a box's paint or the one over a material, whose
    /// kinds the caller checked.
    #[track_caller]
    fn check_fill(&self, _paint: &Paint) {}

    /// Panic unless this shape draws an effect that reads as `read`.
    #[track_caller]
    fn check_effect(&self, _read: ErasedRead<'_>) {}
}

impl<S: ClosedShape, P: Into<Paint>> DrawWith<P> for S {
    type Prepared = Paint;

    fn prepare(paint: P) -> Paint {
        paint.into()
    }

    #[track_caller]
    fn check(&self, paint: &Paint) {
        paint.validate();
        self.check_fill(paint);
    }

    fn draw<'a>(&'a self, paint: &'a Paint) -> Draw<'a> {
        Draw::Closed {
            geometry: self.closed_geometry(),
            paint: ClosedPaintRef::Fill {
                paint,
                material: None,
            },
        }
    }
}

impl<S: ClosedShape> DrawWith<MaterialPaint> for S {
    type Prepared = MaterialPaint;

    fn prepare(paint: MaterialPaint) -> MaterialPaint {
        paint
    }

    #[track_caller]
    fn check(&self, paint: &MaterialPaint) {
        paint.validate();
        self.check_fill(&paint.paint);
    }

    fn draw<'a>(&'a self, paint: &'a MaterialPaint) -> Draw<'a> {
        Draw::Closed {
            geometry: self.closed_geometry(),
            paint: ClosedPaintRef::Fill {
                paint: &paint.paint,
                material: Some(&paint.material),
            },
        }
    }
}

impl<S: ClosedShape> DrawWith<Material> for S {
    type Prepared = MaterialPaint;

    fn prepare(material: Material) -> MaterialPaint {
        MaterialPaint::from(material)
    }

    #[track_caller]
    fn check(&self, paint: &MaterialPaint) {
        <Self as DrawWith<MaterialPaint>>::check(self, paint);
    }

    fn draw<'a>(&'a self, paint: &'a MaterialPaint) -> Draw<'a> {
        <Self as DrawWith<MaterialPaint>>::draw(self, paint)
    }
}
