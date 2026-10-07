//! A stroked shape's cull bound and optional spin, shared by the polyline and curve payloads.

use crate::primitives::geometry::rect::Rect;
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    pub(crate) pivot: Vec2,
    /// Radians, applied to each point before the ancestor transform.
    pub(crate) angle: f32,
}

impl Spin {
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

/// Cull bound plus optional spin; spun shapes cull against the rotation-invariant square about the pivot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StrokeBounds {
    Still(Rect),
    /// Bounding square of the disc swept about `spin.pivot`; stroke reach is applied later, in physical space.
    Spun {
        spin: Spin,
        radius: f32,
    },
}

impl Default for StrokeBounds {
    fn default() -> Self {
        Self::Still(Rect::default())
    }
}

impl StrokeBounds {
    #[inline]
    pub(crate) fn new(owner_rect: Rect, bbox: Rect, rotation: f32) -> Self {
        if rotation == 0.0 {
            return Self::Still(bbox);
        }
        let pivot = owner_rect.spin_pivot();
        Self::Spun {
            spin: Spin {
                pivot,
                angle: rotation,
            },
            radius: bbox.spun_radius(pivot),
        }
    }

    #[inline]
    pub(crate) const fn cull_rect(self) -> Rect {
        match self {
            Self::Still(bbox) => bbox,
            Self::Spun { spin, radius } => Rect::square_about(spin.pivot, radius),
        }
    }

    #[inline]
    pub(crate) const fn spin(self) -> Option<Spin> {
        match self {
            Self::Still(_) => None,
            Self::Spun { spin, .. } => Some(spin),
        }
    }
}
