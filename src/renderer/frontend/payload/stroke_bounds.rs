//! A stroked shape's cull bound and optional spin, shared by the polyline and curve payloads.

use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::payload::spin::Spin;

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
        match Spin::about_owner(owner_rect, owner_rect.min, rotation) {
            None => Self::Still(bbox),
            Some(spin) => Self::Spun {
                spin,
                radius: bbox.spun_radius(spin.pivot),
            },
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
