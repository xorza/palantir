//! [`ArcGeometry`]: an arc record's geometry, which resolves against its owner where it is drawn.

use crate::common::hash::Hasher;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::libm_ext::LibmVec2;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::line_style::LineCap;
use crate::shape::geometry::curve_basis::CurveBasis;
use crate::shape::geometry::placed_stroke::PlacedStroke;
use glam::Vec2;

/// An arc of the largest circle centered in `local_rect`, the owner's arranged rect where `None`,
/// from `start_angle` turning by `sweep`, at most one full circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ArcGeometry {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) start_angle: f32,
    pub(crate) sweep: f32,
}

impl ArcGeometry {
    /// What the arc draws in an owner of `owner_size`, ended by `cap`, `width` across; an arc of
    /// no length, its radius or its sweep rounding to none, paints its cap.
    pub(crate) fn place(self, owner_size: Size, cap: LineCap, width: f32) -> PlacedStroke {
        let rect = self.local_rect.unwrap_or(Rect {
            min: Vec2::ZERO,
            size: owner_size,
        });
        let arc = CircularArc::inscribed(rect, self.start_angle, self.sweep);
        let point = if is_invisible(arc.radius) {
            arc.center
        } else if is_invisible(arc.sweep.abs()) {
            arc.center + arc.radius * Vec2::libm_from_angle(arc.start_angle)
        } else {
            return PlacedStroke::Curve(CurveBasis::Arc(arc));
        };
        PlacedStroke::dot(point, cap, width)
    }

    /// Feed the geometry into a record's hash.
    pub(crate) fn hash_into(self, h: &mut Hasher) {
        self.local_rect.hash_visual(h);
        self.start_angle.hash_visual(h);
        self.sweep.hash_visual(h);
    }
}

impl NanCheck for ArcGeometry {
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.start_angle.is_nan() || self.sweep.is_nan()
    }
}

#[cfg(test)]
mod tests;
