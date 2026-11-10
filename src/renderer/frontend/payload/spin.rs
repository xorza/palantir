//! A paint animation's turn as a payload carries it: a pivot and an angle.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::libm_ext::LibmVec2;
use glam::Vec2;

/// Logical px, as the encoder places a payload: the composer maps it to a draw row's
/// [`RowSpin`](crate::renderer::render_buffer::row_spin::RowSpin). One frame for every payload:
/// one with an `origin` of its own turns its points about [`Self::relative_to`] that origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    /// The owner box's center, in the encoder's logical frame.
    pub(crate) pivot: Vec2,
    /// Radians, applied to each point before the ancestor transform.
    pub(crate) angle: f32,
}

impl Spin {
    /// The turn `rotation` radians about `owner_rect`'s center; `None` when there is no turn.
    #[inline]
    pub(crate) fn about_owner(owner_rect: Rect, rotation: f32) -> Option<Self> {
        (rotation != 0.0).then(|| Self {
            pivot: owner_rect.min + owner_rect.spin_pivot(),
            angle: rotation,
        })
    }

    /// This turn with its pivot relative to `origin`, for points a payload holds from it.
    #[inline]
    pub(crate) fn relative_to(self, origin: Vec2) -> Self {
        Self {
            pivot: self.pivot - origin,
            ..self
        }
    }

    /// Rotation with `sin`/`cos` computed once, since callers turn many points.
    #[inline]
    pub(crate) fn rotor(self) -> SpinRotor {
        SpinRotor {
            rotor: Vec2::libm_from_angle(self.angle),
            pivot: self.pivot,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpinRotor {
    rotor: Vec2,
    pivot: Vec2,
}

impl SpinRotor {
    #[inline]
    pub(crate) fn apply(self, q: Vec2) -> Vec2 {
        self.rotor.rotate(q - self.pivot) + self.pivot
    }
}
