//! [`RectShape`]: a rounded rect, the geometry a box, an image, an icon and an effect fill.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint::Paint;
use crate::shape::PaintedBy;
use crate::shape::draw::ClosedPaintRef;
use crate::shape::draw::Draw;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::sealed;

/// A rounded rect: the owner's arranged rect or [`Self::at`] a rect of its own, and its corner
/// radii. A [`Paint`], a [`MaterialPaint`], an effect, an image or an icon covers it
/// ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct RectShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
}

impl RectShape {
    pub(super) const fn new() -> Self {
        Self {
            local_rect: None,
            corners: Corners::ZERO,
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
        self.local_rect = Some(rect);
        self
    }

    /// Round the corners. Takes one number for all four, or a [`Corners`].
    ///
    /// # Panics
    ///
    /// Panics unless every radius is a [length](crate::widget::domain::is_length) and a
    /// [lane](crate::widget::domain::is_lane).
    #[track_caller]
    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        let corners = corners.into();
        corners.validate();
        self.corners = corners;
        self
    }

    /// The box this rect lays out: its rect and its corners.
    pub(crate) const fn geometry(&self) -> BoxGeometry {
        BoxGeometry {
            local_rect: self.local_rect,
            outline: BoxOutline::Rect(self.corners),
        }
    }

    /// Panic unless the rect is square-cornered, as `what`'s pipeline, which clips to no corners,
    /// draws.
    #[track_caller]
    pub(crate) fn assert_plain(&self, what: &str) {
        assert!(
            self.corners == Corners::ZERO,
            "{what} paints a square-cornered rect"
        );
    }
}

impl<P: Into<Paint>> PaintedBy<P> for RectShape {}

/// A box of `P`: its fill, border and shadow over the rect.
impl<P: Into<Paint>> sealed::DrawWith<P> for RectShape {
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

impl PaintedBy<MaterialPaint> for RectShape {}

/// A box of `MaterialPaint`: its fill, border and shadow over its material, over the rect.
impl sealed::DrawWith<MaterialPaint> for RectShape {
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

impl PaintedBy<Material> for RectShape {}

/// A box of a material alone, as [`MaterialPaint`] draws it.
impl sealed::DrawWith<Material> for RectShape {
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
