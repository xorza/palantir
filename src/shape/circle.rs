//! [`CircleShape`]: the largest circle centered in a rect.

use std::f32::consts::TAU;

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::PaintedBy;
use crate::shape::draw::ClosedPaintRef;
use crate::shape::draw::Draw;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::sealed;

/// The largest circle centered in the owner's arranged rect, or [`Self::at`] a rect of its own,
/// as SwiftUI's `Circle` fits the rect it is offered and WPF's `Ellipse` fills its slot. A
/// [`Paint`] covers it as a round box, its border inside its edge; a [`Stroke`] draws it as a
/// closed arc, centered on the circle; an [`ImagePaint`](crate::ImagePaint) fits its
/// square and the circle clips it, as an [`EffectPaint`](crate::EffectPaint) is.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct CircleShape {
    pub(crate) local_rect: Option<Rect>,
}

impl CircleShape {
    pub(super) const fn new() -> Self {
        Self { local_rect: None }
    }

    /// The round box this circle lays out.
    pub(crate) const fn geometry(&self) -> BoxGeometry {
        BoxGeometry {
            local_rect: self.local_rect,
            outline: BoxOutline::Circle,
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }
}

impl<P: Into<Paint>> PaintedBy<P> for CircleShape {}

/// A round box of `P`.
impl<P: Into<Paint>> sealed::DrawWith<P> for CircleShape {
    type Prepared = Paint;

    fn prepare(paint: P) -> Paint {
        paint.into()
    }

    #[track_caller]
    fn check(&self, paint: &Paint) {
        paint.validate();
    }

    fn draw<'a>(&'a self, paint: &'a Paint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Fill {
                paint,
                material: None,
            },
        }
    }
}

impl PaintedBy<MaterialPaint> for CircleShape {}

/// A round box of `MaterialPaint`.
impl sealed::DrawWith<MaterialPaint> for CircleShape {
    type Prepared = MaterialPaint;

    fn prepare(paint: MaterialPaint) -> MaterialPaint {
        paint
    }

    #[track_caller]
    fn check(&self, paint: &MaterialPaint) {
        paint.validate();
    }

    fn draw<'a>(&'a self, paint: &'a MaterialPaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Fill {
                paint: &paint.paint,
                material: Some(&paint.material),
            },
        }
    }
}

impl PaintedBy<Material> for CircleShape {}

/// A box of a material alone, as [`MaterialPaint`] draws it.
impl sealed::DrawWith<Material> for CircleShape {
    type Prepared = MaterialPaint;

    fn prepare(material: Material) -> MaterialPaint {
        MaterialPaint::from(material)
    }

    #[track_caller]
    fn check(&self, paint: &MaterialPaint) {
        <Self as sealed::DrawWith<MaterialPaint>>::check(self, paint);
    }

    fn draw<'a>(&'a self, paint: &'a MaterialPaint) -> Draw<'a> {
        <Self as sealed::DrawWith<MaterialPaint>>::draw(self, paint)
    }
}

impl PaintedBy<Stroke> for CircleShape {}

/// A closed arc drawn with a [`Stroke`].
impl sealed::DrawWith<Stroke> for CircleShape {
    type Prepared = Stroke;

    fn prepare(paint: Stroke) -> Stroke {
        paint
    }

    #[track_caller]
    fn check(&self, stroke: &Stroke) {
        stroke.validate();
    }

    fn draw<'a>(&'a self, stroke: &'a Stroke) -> Draw<'a> {
        let Self { local_rect } = *self;
        Draw::Arc {
            geometry: ArcGeometry {
                local_rect,
                start_angle: 0.0,
                sweep: TAU,
            },
            stroke,
        }
    }
}
