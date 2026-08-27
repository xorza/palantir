//! Conservative bounding boxes for stroked geometry — how far a cap and a
//! join can push a path's extent past the path itself.

use crate::primitives::rect::Rect;
use crate::shape::style::{LineCap, LineJoin};

/// Half-width of the antialiasing fringe every stroke adds beyond its core
/// half-width, in physical pixels. The curve shader specializes the same value.
pub(crate) const HALF_FRINGE: f32 = 0.5;

/// SVG-convention miter limit shared by CPU bounds, composition, and the
/// specialized curve shader.
pub(crate) const MITER_LIMIT: f32 = 4.0;

/// Conservative paint bound for a centerline AABB. `width` and `fringe` use
/// the same coordinate space as `centerline`.
pub(crate) fn stroked_bbox(
    centerline: Rect,
    width: f32,
    fringe: f32,
    cap: LineCap,
    join: Option<LineJoin>,
) -> Rect {
    let cap_factor = match cap {
        LineCap::Square => std::f32::consts::SQRT_2,
        LineCap::Butt | LineCap::Round => 1.0,
    };
    let join_factor = match join {
        Some(LineJoin::Miter) => MITER_LIMIT,
        Some(LineJoin::Bevel | LineJoin::Round) | None => 1.0,
    };
    let pad = ((width * 0.5).max(0.0) + fringe.max(0.0)) * cap_factor.max(join_factor);
    centerline.inflated(pad)
}

#[cfg(test)]
mod tests;
