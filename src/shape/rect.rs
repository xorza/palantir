use crate::primitives::brush::Brush;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::shape::local_rect_paint_empty;

#[derive(Clone, Copy, Debug)]
pub(crate) enum RectKind {
    Rounded,
    Windowed,
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
    pub(crate) fn new(kind: RectKind, local_rect: Option<Rect>) -> Self {
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

    pub(crate) fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || (self.fill.is_noop() && self.stroke.is_noop())
    }
}
