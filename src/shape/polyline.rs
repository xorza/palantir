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
// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for PolylineShape<'_> {
    fn is_noop(&self) -> bool {
        // Cardinality is a caller contract, not a paint question, so it
        // is asserted rather than filtered — and asserted here because
        // this is the first thing `Shapes::add` calls.
        self.colors.assert_matches(self.points.len());
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
        lower::polyline(
            store,
            self.points,
            self.colors,
            self.width,
            self.cap,
            self.join,
        )
    }
}
