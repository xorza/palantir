use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::shape::local_rect_paint_empty;

/// Gaussian-blurred rounded rectangle shadow.
#[derive(Clone, Debug)]
pub struct ShadowShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) shadow: Shadow,
}

impl ShadowShape {
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        self.corners = corners.into();
        self
    }

    pub(crate) fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.shadow.is_noop()
    }
}
