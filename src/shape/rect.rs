//! [`RectShape`]: a rounded rect, the geometry a box, an image, an icon and an effect fill.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::shape::draw::Draw;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::sealed;
use crate::shape::{IntoPaint, PaintedBy};

/// A rounded rect: the owner's arranged rect or [`Self::at`] a rect of its own, its corner radii,
/// and the side of its boundary it covers. A [`Paint`](crate::Paint), an effect, an image or an icon covers it
/// ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct RectShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) coverage: Coverage,
}

impl RectShape {
    pub(super) const fn new() -> Self {
        Self {
            local_rect: None,
            corners: Corners::ZERO,
            coverage: Coverage::Inside,
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

    /// Round the corners. Takes one number for all four, or a [`Corners`].
    ///
    /// # Panics
    ///
    /// Panics unless every radius is a [length](crate::widget::domain::length) and a
    /// [lane](crate::widget::domain::lane).
    #[track_caller]
    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        let corners = corners.into();
        corners.validate();
        self.corners = corners;
        self
    }

    /// Cover `coverage`, the inside of the boundary by default.
    pub const fn coverage(mut self, coverage: Coverage) -> Self {
        self.coverage = coverage;
        self
    }

    /// The box this rect lays out: its rect, its corners and the side it covers.
    pub(crate) const fn geometry(&self) -> BoxGeometry {
        BoxGeometry {
            local_rect: self.local_rect,
            outline: BoxOutline::Rect(self.corners),
            coverage: self.coverage,
        }
    }

    /// Panic unless the rect is square-cornered and covers its inside, as `what`'s pipeline,
    /// which clips to neither corners nor a side, draws.
    #[track_caller]
    pub(crate) fn assert_plain(&self, what: &str) {
        assert!(
            self.corners == Corners::ZERO,
            "{what} paints a square-cornered rect"
        );
        self.assert_inside(what);
    }

    /// Panic unless the rect covers its inside, as `what`'s pipeline, which clips to no side,
    /// draws.
    #[track_caller]
    pub(crate) fn assert_inside(&self, what: &str) {
        assert!(
            self.coverage == Coverage::Inside,
            "{what} paints the inside of its rect"
        );
    }
}

impl<P: IntoPaint> PaintedBy<P> for RectShape {}

/// A box of `P`: its fill, border, shadow and material over the rect.
impl<P: IntoPaint> sealed::DrawWith<P> for RectShape {
    #[track_caller]
    fn check(&self, paint: &P) {
        paint.to_paint().validate();
    }

    fn draw<'a>(&'a self, paint: &'a P) -> Draw<'a> {
        Draw::Box {
            geometry: self.geometry(),
            paint: paint.to_paint(),
        }
    }
}
