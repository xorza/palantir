//! The polyline builder and its per-vertex or per-segment color source.

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::record::{ColorMode, ShapeRecord};
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;
use std::hash::Hasher as _;
use std::slice;

/// Stroked polyline, centered on its points, in one color or varied per point or segment.
#[derive(Clone, Debug)]
#[must_use]
pub struct PolylineShape<'a> {
    pub(crate) points: &'a [Vec2],
    pub(crate) stroke: Stroke,
    pub(crate) colors: PolylineColors<'a>,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    /// The points' AABB, folded once at construction so [`Shape::polyline`](crate::widget::Shape::polyline) checks every point in one test.
    pub(crate) bbox: Rect,
}

impl<'a> PolylineShape<'a> {
    pub(super) fn new(points: &'a [Vec2], stroke: Stroke) -> Self {
        Self {
            points,
            stroke,
            colors: PolylineColors::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            // Under the AABB NaN contract, so a NaN point lands in the bbox.
            bbox: Aabb::of(points),
        }
    }

    /// One color per point, lerped between points; multiplies the stroke color per channel.
    ///
    /// # Panics
    ///
    /// Panics unless `colors` has one entry per point and every color is a
    /// [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn per_point(mut self, colors: &'a [RgbaF32]) -> Self {
        assert!(
            colors.len() == self.points.len(),
            "per-point polyline colors must number the points",
        );
        check_colors(colors);
        self.colors = PolylineColors::PerPoint(colors);
        self
    }

    /// One color per segment, each a solid block; multiplies the stroke color.
    ///
    /// # Panics
    ///
    /// Panics unless `colors` has one entry per segment, one fewer than the points, and every
    /// color is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn per_segment(mut self, colors: &'a [RgbaF32]) -> Self {
        assert!(
            colors.len() == self.points.len().saturating_sub(1),
            "per-segment polyline colors must number the segments, one fewer than the points",
        );
        check_colors(colors);
        self.colors = PolylineColors::PerSegment(colors);
        self
    }

    /// Line end cap.
    pub const fn cap(mut self, cap: LineCap) -> Self {
        self.cap = cap;
        self
    }

    /// Corner join.
    pub const fn join(mut self, join: LineJoin) -> Self {
        self.join = join;
        self
    }
}

/// Where a polyline's color varies, on top of its stroke color.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PolylineColors<'a> {
    Single,
    /// One multiplier per input point; the GPU lerps between them.
    PerPoint(&'a [RgbaF32]),
    /// One multiplier per segment; no color bleeds across a joint.
    PerSegment(&'a [RgbaF32]),
}

#[track_caller]
const fn check_colors(colors: &[RgbaF32]) {
    let mut i = 0;
    while i < colors.len() {
        let _ = domain::color(colors[i]);
        i += 1;
    }
}

impl PolylineColors<'_> {
    /// Whether the colors fit `points_len` points, as [`PolylineShape::per_point`] and
    /// [`PolylineShape::per_segment`] assert.
    const fn matches(&self, points_len: usize) -> bool {
        match self {
            PolylineColors::Single => true,
            PolylineColors::PerPoint(colors) => colors.len() == points_len,
            PolylineColors::PerSegment(colors) => colors.len() == points_len.saturating_sub(1),
        }
    }
}

impl sealed::LowerShape for PolylineShape<'_> {
    fn is_noop(&self) -> bool {
        if self.stroke.is_noop() || self.points.len() < 2 {
            return true;
        }
        match self.colors {
            PolylineColors::Single => false,
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                colors.iter().all(|color| color.is_noop())
            }
        }
    }

    /// Points into the store, colors multiplied by the stroke color, then the content hash. A single stroke lowers to `ShapeRecord::Curve` instead.
    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            points,
            stroke,
            colors,
            cap,
            join,
            bbox,
        } = self;
        let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
            PolylineColors::Single => (ColorMode::Single, slice::from_ref(&RgbaF32::WHITE)),
            PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
            PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
        };

        // `Shapes::add` drops < 2-point polylines via `is_noop`, and the color setters checked the count.
        debug_assert!(
            points.len() >= 2,
            "polyline with < 2 points reached lowering"
        );
        debug_assert!(colors.matches(points.len()));
        let staged = store.stage_polyline(points, color_slice, stroke.color);
        let lowered_colors = &store.polyline_colors[staged.colors.range()];

        let mut h = Hasher::new();
        for &point in points {
            point.hash_visual(&mut h);
        }
        h.pod_slice(lowered_colors);
        let style = (u64::from(float_hash::canon_bits(stroke.width)) << 24)
            | ((mode as u64) << 16)
            | ((cap as u64) << 8)
            | (join as u64);
        h.write_u64(style);
        let content_hash = h.finish();

        ShapeRecord::Polyline {
            width: stroke.width,
            color_mode: mode,
            cap,
            join,
            points: staged.points,
            colors: staged.colors,
            bbox,
            content_hash,
        }
    }
}
