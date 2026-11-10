//! [`CircleShape`]: the largest circle centered in a rect.

use std::f32::consts::TAU;

use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::draw::ClosedPaintRef;
use crate::shape::draw::Draw;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::box_geometry::{BoxGeometry, BoxOutline};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::sealed;
use crate::shape::{IntoPaint, PaintedBy};

/// The largest circle centered in the owner's arranged rect, or [`Self::at`] a rect of its own,
/// as SwiftUI's `Circle` fits the rect it is offered and WPF's `Ellipse` fills its slot. A
/// [`Paint`](crate::Paint) covers it as a round box, its border inside its edge; a [`Stroke`] draws it as a
/// closed arc, centered on the circle; an [`ImagePaint`](crate::widget::ImagePaint) fits its
/// square and the circle clips it, as an [`EffectPaint`](crate::widget::EffectPaint) is.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct CircleShape {
    pub(crate) local_rect: Option<Rect>,
}

impl CircleShape {
    pub(super) const fn new() -> Self {
        Self { local_rect: None }
    }

    /// The round box this circle lays out, covering its inside.
    pub(crate) const fn geometry(&self) -> BoxGeometry {
        BoxGeometry {
            local_rect: self.local_rect,
            outline: BoxOutline::Circle,
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
}

impl<P: IntoPaint> PaintedBy<P> for CircleShape {}

/// A round box of `P`.
impl<P: IntoPaint> sealed::DrawWith<P> for CircleShape {
    #[track_caller]
    fn check(&self, paint: &P) {
        paint.to_paint().validate();
    }

    fn draw<'a>(&'a self, paint: &'a P) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Fill(paint.to_paint()),
        }
    }
}

impl PaintedBy<Stroke> for CircleShape {}

/// A closed arc drawn with a [`Stroke`].
impl sealed::DrawWith<Stroke> for CircleShape {
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
