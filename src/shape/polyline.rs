//! The polyline builder and its per-vertex or per-segment color source.
//! Lowers to `ShapeRecord::Polyline` — the one stroke with interior joins,
//! which is what separates it from the single strokes in `curve`.

use crate::primitives::color::RgbaF32;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::rect::aabb::Aabb;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;

/// Stroked polyline, centred on its points like every path shape's
/// stroke, in one colour or with it varied per point or per segment.
#[derive(Clone, Debug)]
pub struct PolylineShape<'a> {
    pub(crate) points: &'a [Vec2],
    pub(crate) stroke: Stroke,
    pub(crate) colors: PolylineColors<'a>,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
    /// The points' AABB, folded once here rather than during lowering.
    ///
    /// The one bulk input in the crate that is a borrowed slice with no
    /// owner to memoize on — a `Mesh` caches its own. Folding at
    /// construction is what lets [`sealed::LowerShape::has_nan`] answer
    /// in `O(1)` like every other kind, and the record needs the same
    /// bbox afterwards, so it is one fold either way. The cost is folding
    /// a polyline that later turns out to paint nothing.
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
            // Under the AABB NaN contract, so a NaN point lands in the
            // bbox rather than being lost to `f32::min`'s NaN behaviour.
            bbox: Aabb::of(points),
        }
    }

    /// One colour per point, lerped between adjacent points: a smooth
    /// gradient along the line. Each multiplies the stroke colour,
    /// channel by channel — the rule a mesh tint follows. `colors.len()`
    /// must equal the number of points.
    pub fn per_point(mut self, colors: &'a [RgbaF32]) -> Self {
        self.colors = PolylineColors::PerPoint(colors);
        self
    }

    /// One colour per segment, each a solid block (join chrome blends the
    /// two neighbours), multiplying the stroke colour like
    /// [`Self::per_point`]. `colors.len()` must be one less than the
    /// number of points.
    pub fn per_segment(mut self, colors: &'a [RgbaF32]) -> Self {
        self.colors = PolylineColors::PerSegment(colors);
        self
    }

    /// How the two open ends are finished.
    pub fn cap(mut self, cap: impl Into<LineCap>) -> Self {
        self.cap = cap.into();
        self
    }

    /// How interior corners are finished.
    pub fn join(mut self, join: impl Into<LineJoin>) -> Self {
        self.join = join.into();
        self
    }
}

/// Where a polyline's colour varies, on top of its stroke colour.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PolylineColors<'a> {
    /// The stroke colour alone, broadcast to every cross-section.
    Single,
    /// One multiplier per input point; `len()` equals `points.len()`.
    /// The GPU lerps between adjacent cross-sections.
    PerPoint(&'a [RgbaF32]),
    /// One multiplier per segment; `len()` equals `points.len() - 1`.
    /// No colour bleeds across a joint.
    PerSegment(&'a [RgbaF32]),
}

impl PolylineColors<'_> {
    /// Check the per-point / per-segment cardinality contract.
    ///
    /// One length compare, run per polyline per frame against a `memcpy`
    /// of the points and a hash of every one of them — which is why it is
    /// affordable in release, where the miscount it catches is not
    /// recoverable: lowering stages a colour slice of the wrong length,
    /// and the composer reads per-point colours off the end of it.
    pub(crate) fn assert_matches(&self, points_len: usize) {
        match self {
            PolylineColors::Single => {}
            PolylineColors::PerPoint(colors) => assert_eq!(
                colors.len(),
                points_len,
                "Shape::Polyline PerPoint colors len {} != points len {}",
                colors.len(),
                points_len,
            ),
            PolylineColors::PerSegment(colors) => assert_eq!(
                colors.len(),
                points_len.saturating_sub(1),
                "Shape::Polyline PerSegment colors len {} != points len - 1 ({})",
                colors.len(),
                points_len.saturating_sub(1),
            ),
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

    /// The per-point and per-segment colours are the bulk input `bbox`
    /// does not cover, and they are `RgbaF32`s rather than positions — a
    /// NaN channel reads as invisible through `is_noop` above rather than
    /// poisoning geometry, so the fold that would scan them buys nothing.
    fn has_nan(&self) -> bool {
        self.stroke.has_nan() || self.bbox.has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            points,
            stroke,
            colors,
            cap,
            join,
            bbox,
        } = self;
        lower::polyline(store, points, stroke, colors, cap, join, bbox)
    }
}
