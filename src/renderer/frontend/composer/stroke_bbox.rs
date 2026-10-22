//! The painted bounds a stroked shape is culled and ordered by.

use crate::display::Display;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::frontend::composer::geometry;
use crate::renderer::frontend::composer::physical_map::PhysicalMap;
use crate::shape::stroke_bounds::{StrokeJoints, StrokeReach};
use crate::shape::style::LineCap;
use glam::Vec2;

/// One stroked shape's owner-local centerline and the style laid over it.
/// Named fields because the curve and polyline paths differ in two and agree
/// on the rest.
#[derive(Clone, Copy, Debug)]
pub(super) struct StrokeBbox {
    pub(super) transform: TranslateScale,
    /// Owner-local centerline bounds, before `origin` and `transform`.
    pub(super) bbox: Rect,
    pub(super) origin: Vec2,
    pub(super) width_physical: f32,
    pub(super) cap: LineCap,
    pub(super) joints: StrokeJoints,
    pub(super) display: Display,
}

impl StrokeBbox {
    /// Physical-px painted bounds: folds in `origin` and the active transform,
    /// applies the shared stroke/cap/join/AA bound, clamps to the viewport. Shared
    /// by curve and polyline paths so their cull and overlap bounds agree.
    pub(super) fn urect(self) -> URect {
        let Self {
            transform,
            bbox,
            origin,
            width_physical,
            cap,
            joints,
            display,
        } = self;
        let centerline_physical =
            PhysicalMap::new(transform, origin, display.scale_factor()).bbox(bbox);
        let painted = StrokeReach {
            width: width_physical,
            fringe: AA_HALF_WIDTH,
            cap,
            joints,
        }
        .bbox(centerline_physical);
        geometry::urect_from_physical(painted.min, painted.max(), display.physical)
    }
}
