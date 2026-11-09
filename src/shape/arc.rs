//! [`ArcShape`]: an arc of the largest circle centered in a rect.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::PaintedBy;
use crate::shape::draw::Draw;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::sealed;

/// An arc of the largest circle centered in the owner's arranged rect, or [`Self::at`] a rect of
/// its own, from its start angle turning by its sweep, as Compose's `drawArc` runs on its rect's
/// oval. A [`Stroke`] draws it, centered on the circle.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct ArcShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) start_angle: f32,
    /// Coerced to one full circle, `±2π`, its sign kept: a longer sweep traces the same circle.
    pub(crate) sweep: f32,
}

impl ArcShape {
    pub(super) const fn new(start_angle: f32, sweep: f32) -> Self {
        Self {
            local_rect: None,
            start_angle,
            sweep: sweep.clamp(-std::f32::consts::TAU, std::f32::consts::TAU),
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

impl PaintedBy<Stroke> for ArcShape {}

/// An arc drawn with a [`Stroke`].
impl sealed::DrawWith<Stroke> for ArcShape {
    #[track_caller]
    fn check(&self, stroke: &Stroke) {
        stroke.validate();
    }

    fn draw<'a>(&'a self, stroke: &'a Stroke) -> Draw<'a> {
        let Self {
            local_rect,
            start_angle,
            sweep,
        } = *self;
        Draw::Arc {
            geometry: ArcGeometry {
                local_rect,
                start_angle,
                sweep,
            },
            stroke,
        }
    }
}
