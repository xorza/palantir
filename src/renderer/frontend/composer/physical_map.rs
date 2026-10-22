//! How owner-local geometry lands in physical pixels.

use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::translate_scale::TranslateScale;
use glam::Vec2;

/// The map from owner-local logical px to physical px: owner origin, active
/// transform, display factor. Built once per draw, so the transform read sits
/// outside callers' per-point loops.
#[derive(Clone, Copy, Debug)]
pub(super) struct PhysicalMap {
    transform: TranslateScale,
    origin: Vec2,
    display_scale: f32,
}

impl PhysicalMap {
    pub(super) const fn new(transform: TranslateScale, origin: Vec2, display_scale: f32) -> Self {
        Self {
            transform,
            origin,
            display_scale,
        }
    }

    /// An owner-local point in physical px.
    #[inline]
    pub(super) fn point(self, q: Vec2) -> Vec2 {
        self.transform.apply_point(q + self.origin) * self.display_scale
    }

    /// An owner-local bbox in physical px. Unsnapped, since mesh and stroked
    /// tiers place sub-pixel geometry and let shaders resolve the fringe (see
    /// [`StrokeBbox::urect`](crate::renderer::frontend::composer::stroke_bbox::StrokeBbox::urect)).
    #[inline]
    pub(super) fn bbox(self, bbox: Rect) -> Rect {
        self.transform
            .apply_rect(Rect {
                min: bbox.min + self.origin,
                size: bbox.size,
            })
            .scaled_by(self.display_scale, EdgeRounding::Exact)
    }
}
