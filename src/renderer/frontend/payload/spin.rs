//! A paint animation's turn as a payload and a draw row carry it: a pivot and an angle.

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    /// The owner box's center: in a payload, in the frame its points are placed in; in a draw
    /// row, in physical px.
    pub(crate) pivot: Vec2,
    /// Radians, applied to each point before the ancestor transform.
    pub(crate) angle: f32,
}

impl Spin {
    /// The turn `rotation` radians about `owner_rect`'s center, for a payload whose points are
    /// relative to `origin`; `None` when there is no turn.
    #[inline]
    pub(crate) fn about_owner(owner_rect: Rect, origin: Vec2, rotation: f32) -> Option<Self> {
        (rotation != 0.0).then(|| Self {
            pivot: (owner_rect.min - origin) + owner_rect.spin_pivot(),
            angle: rotation,
        })
    }

    /// Rotation with `sin`/`cos` computed once, since callers turn many points.
    #[inline]
    pub(crate) fn rotor(self) -> SpinRotor {
        SpinRotor {
            rotor: Vec2::from_angle(self.angle),
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
