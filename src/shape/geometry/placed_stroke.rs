//! [`PlacedStroke`]: what a curve or an arc record draws once it is placed in its owner.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::line_style::LineCap;
use crate::shape::geometry::box_geometry::{BoxOutline, PlacedBox};
use crate::shape::geometry::curve_basis::CurveBasis;
use glam::Vec2;

/// What a native stroke draws in its owner, owner-local: one placement the cascade bounds and the
/// encoder draws, so damage and paint agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PlacedStroke {
    /// The stroke, traced on the curve pipeline.
    Curve(CurveBasis),
    /// The cap of a stroke of no length: a disc under a round cap, a square under a square one,
    /// the stroke width across, as SVG paints a zero-length subpath.
    Dot(PlacedBox),
    /// A butt-capped stroke of no length, which has no extent.
    Nothing,
}

impl PlacedStroke {
    /// The cap a stroke of no length at `point` paints, ended by `cap`, `width` across.
    pub(crate) fn dot(point: Vec2, cap: LineCap, width: f32) -> Self {
        let outline = match cap {
            LineCap::Butt => return Self::Nothing,
            LineCap::Round => BoxOutline::Circle,
            LineCap::Square => BoxOutline::Rect(Corners::ZERO),
        };
        let half = 0.5 * width;
        Self::Dot(outline.place(Rect::new(point.x - half, point.y - half, width, width)))
    }
}
