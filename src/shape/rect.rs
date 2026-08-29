//! The rectangle builder. Lowers to `ShapeRecord::Quad(QuadShape::Rect)`.

use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RectKind {
    /// Fill inside the rounded boundary.
    Rounded = 0,
    /// Fill outside the rounded boundary, leaving its interior transparent.
    Windowed = 1,
}

/// Filled and/or stroked rectangle.
#[derive(Clone, Debug)]
pub struct RectShape {
    pub(crate) kind: RectKind,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) fill: Brush,
    pub(crate) stroke: Stroke,
}

impl RectShape {
    pub(super) fn new(kind: RectKind, local_rect: Option<Rect>) -> Self {
        Self {
            kind,
            local_rect,
            corners: Corners::ZERO,
            fill: Brush::TRANSPARENT,
            stroke: Stroke::ZERO,
        }
    }
}

local_rect_shape!(RectShape);

shape_setters!(RectShape {
    fill: Brush => fill,
    stroke: Stroke => stroke,
    corners: Corners => corners,
});

impl sealed::LowerShape for RectShape {
    fn is_noop(&self) -> bool {
        self.rect_is_noop() || (self.fill.is_noop() && self.stroke.is_noop())
    }

    /// `fill` is screened here rather than where it interns: a gradient's
    /// geometry disappears into the store behind a `GradientId`, so this
    /// is the last point at which the record gate could still see it —
    /// and rejecting after the intern would leave the row in the pool.
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan()
            || self.corners.has_nan()
            || self.fill.has_nan()
            || self.stroke.has_nan()
    }

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        let Self {
            kind,
            local_rect,
            corners,
            fill,
            stroke,
        } = self;
        lower::rect(store, kind, local_rect, corners, &fill, stroke)
    }
}
