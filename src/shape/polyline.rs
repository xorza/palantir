//! The polyline builder and its per-vertex or per-segment color source.
//! Lowers to `ShapeRecord::Polyline` — the one stroke with interior joins,
//! which is what separates it from the single strokes in `curve`.

use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
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
}

shape_setters!(PolylineShape<'_> {
    cap: LineCap => cap,
    join: LineJoin => join,
});

/// Color source for [`Shape::polyline`](crate::Shape::polyline).
#[derive(Clone, Copy, Debug)]
pub enum PolylineColors<'a> {
    /// One color for the whole stroke. Broadcast to every cross-section.
    Single(Color),
    /// One color per input point. `len()` must equal `points.len()`.
    /// GPU lerps between adjacent cross-sections, giving a smooth
    /// gradient along the stroke.
    PerPoint(&'a [Color]),
    /// One color per segment. `len()` must equal
    /// `points.len() - 1`. Each segment renders as its own solid
    /// block (join chrome blends the two neighbors) — no color
    /// bleed at joins.
    PerSegment(&'a [Color]),
}

impl PolylineColors<'_> {
    /// Check the per-point / per-segment cardinality contract.
    ///
    /// Debug-only: every polyline authored every frame reaches this, and a
    /// release build must not pay for a caller contract on an immediate-mode
    /// path. Named for what it compiles to so the call site reads honestly.
    pub(crate) fn debug_assert_matches(&self, points_len: usize) {
        match self {
            PolylineColors::Single(_) => {}
            PolylineColors::PerPoint(colors) => debug_assert_eq!(
                colors.len(),
                points_len,
                "Shape::Polyline PerPoint colors len {} != points len {}",
                colors.len(),
                points_len,
            ),
            PolylineColors::PerSegment(colors) => debug_assert_eq!(
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

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        let Self {
            points,
            colors,
            width,
            cap,
            join,
        } = self;
        lower::polyline(store, points, colors, width, cap, join)
    }
}
