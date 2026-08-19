//! A stroked shape's cull bound, and the spin it may carry — shared by
//! the polyline and curve payloads.

use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

/// Where a stroked shape rotates, for the shapes that do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spin {
    /// Owner-local point the shape turns about.
    pub(crate) pivot: Vec2,
    /// Radians, applied to each point before the ancestor transform.
    pub(crate) angle: f32,
}

/// A stroked shape's owner-local cull bound, and its spin if it has one.
///
/// One value rather than a `bbox: Rect` beside a `rotation: f32`,
/// because the two were not independent: a non-zero rotation meant the
/// rect was no longer the centerline AABB but the rotation-invariant
/// square about the pivot, and `bbox.center()` was the only way the
/// composer could recover that pivot once the owner rect was gone.
/// Encoding it here means the still case cannot carry a stale pivot and
/// the spun case cannot lose one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StrokeBounds {
    /// Centerline AABB, owner-local.
    Still(Rect),
    /// A spun shape sweeps a disc about `spin.pivot`, so what it is
    /// culled and batched against is that disc's bounding square —
    /// rotation-invariant, which is what keeps the composer's overlap
    /// tracking correct at every angle. Stroke reach is applied after
    /// it, in physical space.
    Spun { spin: Spin, radius: f32 },
}

impl Default for StrokeBounds {
    fn default() -> Self {
        Self::Still(Rect::default())
    }
}

impl StrokeBounds {
    /// Owner-local rect the composer culls and batches against.
    #[inline]
    pub(crate) fn cull_rect(self) -> Rect {
        match self {
            Self::Still(bbox) => bbox,
            Self::Spun { spin, radius } => Rect {
                min: spin.pivot - Vec2::splat(radius),
                size: Size {
                    w: 2.0 * radius,
                    h: 2.0 * radius,
                },
            },
        }
    }

    /// The spin to draw under, or `None` for the common still case.
    #[inline]
    pub(crate) fn spin(self) -> Option<Spin> {
        match self {
            Self::Still(_) => None,
            Self::Spun { spin, .. } => Some(spin),
        }
    }
}
