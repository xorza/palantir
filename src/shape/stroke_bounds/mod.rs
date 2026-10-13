//! Conservative bounding boxes for stroked geometry: how far a cap and join push a path's extent.

use crate::primitives::geometry::rect::Rect;
use crate::shape::style::{LineCap, LineJoin};
use std::f32::consts;

/// SVG-convention miter limit shared by CPU bounds, composition and the curve shader.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// How far a stroke paints past its centerline. `width` and `fringe` use the
/// same coordinate space as the centerline.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StrokeReach {
    pub(crate) width: f32,
    pub(crate) fringe: f32,
    pub(crate) cap: LineCap,
    pub(crate) joints: StrokeJoints,
}

/// The corners a stroke turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StrokeJoints {
    /// A single segment or a curve: no corner to join.
    Unjoined,
    Joined(LineJoin),
}

impl StrokeJoints {
    /// A polyline of `points` points turns a corner at each interior point.
    pub(crate) const fn polyline(points: u32, join: LineJoin) -> Self {
        if points > 2 {
            Self::Joined(join)
        } else {
            Self::Unjoined
        }
    }
}

impl StrokeReach {
    /// Conservative paint bound for the `centerline` AABB.
    pub(crate) fn bbox(self, centerline: Rect) -> Rect {
        let Self {
            width,
            fringe,
            cap,
            joints,
        } = self;
        let cap_factor = match cap {
            LineCap::Square => consts::SQRT_2,
            LineCap::Butt | LineCap::Round => 1.0,
        };
        let join_factor = match joints {
            StrokeJoints::Joined(LineJoin::Miter) => MITER_LIMIT,
            StrokeJoints::Joined(LineJoin::Bevel | LineJoin::Round) | StrokeJoints::Unjoined => 1.0,
        };
        let pad = ((width * 0.5).max(0.0) + fringe.max(0.0)) * cap_factor.max(join_factor);
        centerline.inflated(pad)
    }
}

#[cfg(test)]
mod tests;
