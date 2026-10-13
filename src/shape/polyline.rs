//! The polyline builder and its per-vertex or per-segment color source.

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::rect::aabb::Aabb;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::record::{ColorMode, ShapeRecord};
use crate::shape::sealed;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;
use std::hash::Hasher as _;
use std::slice;

/// Stroked polyline, centred on its points, in one colour or varied per point or segment.
#[derive(Clone, Debug)]
#[must_use]
pub struct PolylineShape<'a> {
    pub(crate) points: &'a [Vec2],
    pub(crate) stroke: Stroke,
    pub(crate) colors: PolylineColors<'a>,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    /// The points' AABB, folded once at construction so [`sealed::LowerShape::has_nan`] is `O(1)`; a borrowed slice has nothing to memoize on.
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

    /// One colour per point, lerped between points; multiplies the stroke colour per channel. `colors.len()` must equal the point count.
    ///
    /// # Panics
    ///
    /// Panics unless every colour is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn per_point(mut self, colors: &'a [RgbaF32]) -> Self {
        check_colors(colors);
        self.colors = PolylineColors::PerPoint(colors);
        self
    }

    /// One colour per segment, each a solid block; multiplies the stroke colour. `colors.len()` must be one less than the point count.
    ///
    /// # Panics
    ///
    /// As [`Self::per_point`].
    #[track_caller]
    pub const fn per_segment(mut self, colors: &'a [RgbaF32]) -> Self {
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

/// Where a polyline's colour varies, on top of its stroke colour.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PolylineColors<'a> {
    Single,
    /// One multiplier per input point; the GPU lerps between them.
    PerPoint(&'a [RgbaF32]),
    /// One multiplier per segment; no colour bleeds across a joint.
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
    const fn matches(&self, points_len: usize) -> bool {
        match self {
            PolylineColors::Single => true,
            PolylineColors::PerPoint(colors) => colors.len() == points_len,
            PolylineColors::PerSegment(colors) => colors.len() == points_len.saturating_sub(1),
        }
    }

    /// Check the per-point / per-segment cardinality contract; release-checked because the composer would read a short colour slice past its end.
    pub(crate) fn assert_matches(&self, points_len: usize) {
        assert!(
            self.matches(points_len),
            "Shape::Polyline {self:?} does not fit {points_len} points: per-point colours \
             number the points, per-segment colours one fewer",
        );
    }

    const fn colors(&self) -> &[RgbaF32] {
        match self {
            PolylineColors::Single => &[],
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => colors,
        }
    }
}

impl sealed::LowerShape for PolylineShape<'_> {
    /// A wrong-length colour slice is never a no-op, however transparent; lowering's cardinality assert names the bug.
    fn is_noop(&self) -> bool {
        if self.stroke.is_noop() || self.points.len() < 2 {
            return true;
        }
        match self.colors {
            PolylineColors::Single => false,
            _ if !self.colors.matches(self.points.len()) => false,
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                colors.iter().all(|color| color.is_noop())
            }
        }
    }

    /// Colours are scanned, since `bbox` does not cover them: a NaN channel would reach the shader.
    fn has_nan(&self) -> bool {
        self.stroke.has_nan()
            || self.bbox.has_nan()
            || self.colors.colors().iter().any(NanCheck::has_nan)
    }

    /// Points into the store, colours multiplied by the stroke colour, then the content hash. A single stroke lowers to `ShapeRecord::Curve` instead.
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

        // `Shapes::add` drops < 2-point polylines via `is_noop`, so a degenerate slice is a caller bug; colour cardinality is checked here since a query answers, it does not validate.
        debug_assert!(
            points.len() >= 2,
            "polyline with < 2 points reached lowering"
        );
        colors.assert_matches(points.len());
        // `bbox` was folded by `PolylineShape::new` so `Shapes::add` can screen before staging. Two passes beat one interleaved: the fold vectorizes alone and the copy becomes one `memcpy`.
        debug_assert!(
            !bbox.has_nan(),
            "NaN polyline point reached lowering — `Shapes::add` screens the bbox",
        );
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
