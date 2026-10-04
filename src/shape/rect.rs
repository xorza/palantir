//! The rectangle builder. Lowers to `ShapeRecord::Quad(QuadShape::Rect)`.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::shape::lower;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RectKind {
    /// Fill inside the rounded boundary.
    Rounded = 0,
    /// Fill outside the rounded boundary, leaving its interior transparent.
    Windowed = 1,
}

/// Filled and/or bordered rectangle.
#[derive(Clone, Debug)]
#[must_use]
pub struct RectShape {
    pub(crate) kind: RectKind,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) fill: Brush,
    pub(crate) border: Stroke,
}

impl RectShape {
    pub(super) const fn new(kind: RectKind, local_rect: Option<Rect>) -> Self {
        Self {
            kind,
            local_rect,
            corners: Corners::ZERO,
            fill: Brush::TRANSPARENT,
            border: Stroke::ZERO,
        }
    }
}

impl RectShape {
    /// Interior paint.
    ///
    /// # Panics
    ///
    /// Panics unless a solid fill is a [colour](crate::widget::domain::color), and a
    /// gradient's geometry holds the kinds [`Background`](crate::Background)
    /// lists for a fill.
    #[track_caller]
    pub fn fill(mut self, fill: impl Into<Brush>) -> Self {
        let fill = fill.into();
        fill.validate();
        self.fill = fill;
        self
    }

    /// Edge paint, inside the boundary: the outer edge of the border is
    /// the rect's edge, like every area shape's.
    ///
    /// # Panics
    ///
    /// Panics unless the width is a [length](crate::widget::domain::length) and
    /// the colour a [colour](crate::widget::domain::color).
    #[track_caller]
    pub fn border(mut self, border: impl Into<Stroke>) -> Self {
        let border = border.into();
        border.validate();
        self.border = border;
        self
    }

    /// Corner radii. Takes one number for all four, or a [`Corners`].
    ///
    /// # Panics
    ///
    /// Panics unless every radius is a [length](crate::widget::domain::length) of at most
    /// 65504, one f16 lane.
    #[track_caller]
    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        let corners = corners.into();
        corners.validate();
        self.corners = corners;
        self
    }
}

impl sealed::LowerShape for RectShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
            || (self.fill.is_noop() && self.border.is_noop())
    }

    /// `fill` is screened here rather than where it interns: a gradient's
    /// geometry disappears into the store behind a `GradientId`, so this
    /// is the last point at which the record gate could still see it —
    /// and rejecting after the intern would leave the row in the pool.
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan()
            || self.corners.has_nan()
            || self.fill.has_nan()
            || self.border.has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            kind,
            local_rect,
            corners,
            fill,
            border,
        } = self;
        lower::rect(store, kind, local_rect, corners, &fill, border)
    }
}
