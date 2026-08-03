use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::shape::local_rect_paint_empty;

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

    pub fn fill(mut self, fill: impl Into<Brush>) -> Self {
        self.fill = fill.into();
        self
    }

    pub fn stroke(mut self, stroke: impl Into<Stroke>) -> Self {
        self.stroke = stroke.into();
        self
    }

    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        self.corners = corners.into();
        self
    }

    pub(super) fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || (self.fill.is_noop() && self.stroke.is_noop())
    }
}
