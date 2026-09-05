//! The polyline builder and its per-vertex or per-segment color source.
//! Lowers to `ShapeRecord::Polyline` — the one stroke with interior joins,
//! which is what separates it from the single strokes in `curve`.

use crate::primitives::approx::noop_f32;
use crate::primitives::color::RgbaF32;
use crate::primitives::rect::Rect;
use crate::primitives::rect::aabb::Aabb;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;
use crate::shape::style::{LineCap, LineJoin};
use glam::Vec2;

/// Stroked polyline with per-vertex or per-segment coloring.
#[derive(Clone, Debug)]
pub struct PolylineShape<'a> {
    pub(crate) points: &'a [Vec2],
    pub(crate) colors: PolylineColors<'a>,
    pub(crate) width: f32,
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
    pub(super) fn new(points: &'a [Vec2], colors: PolylineColors<'a>, width: f32) -> Self {
        Self {
            points,
            colors,
            width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            // Under the AABB NaN contract, so a NaN point lands in the
            // bbox rather than being lost to `f32::min`'s NaN behaviour.
            bbox: Aabb::of(points),
        }
    }
}

impl PolylineShape<'_> {
    pub fn cap(mut self, cap: impl Into<LineCap>) -> Self {
        self.cap = cap.into();
        self
    }

    pub fn join(mut self, join: impl Into<LineJoin>) -> Self {
        self.join = join.into();
        self
    }
}

/// RgbaF32 source for [`Shape::polyline`](crate::Shape::polyline).
#[derive(Clone, Copy, Debug)]
pub enum PolylineColors<'a> {
    /// One color for the whole stroke. Broadcast to every cross-section.
    Single(RgbaF32),
    /// One color per input point. `len()` must equal `points.len()`.
    /// GPU lerps between adjacent cross-sections, giving a smooth
    /// gradient along the stroke.
    PerPoint(&'a [RgbaF32]),
    /// One color per segment. `len()` must equal
    /// `points.len() - 1`. Each segment renders as its own solid
    /// block (join chrome blends the two neighbors) — no color
    /// bleed at joins.
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
            PolylineColors::Single(_) => {}
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
        if noop_f32(self.width) || self.points.len() < 2 {
            return true;
        }
        match self.colors {
            PolylineColors::Single(color) => color.is_noop(),
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                colors.iter().all(|color| color.is_noop())
            }
        }
    }

    /// The colours are the bulk input `bbox` does not cover, and they
    /// are `RgbaF32`s rather than positions — a NaN channel reads as
    /// invisible through `is_noop` above rather than poisoning geometry,
    /// so the fold that would scan them buys nothing.
    fn has_nan(&self) -> bool {
        self.width.is_nan() || self.bbox.has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            points,
            colors,
            width,
            cap,
            join,
            bbox,
        } = self;
        lower::polyline(store, points, colors, width, cap, join, bbox)
    }
}
