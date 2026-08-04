use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::local_rect_paint_empty;
use crate::shape::sealed;

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
}
// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for ShadowShape {
    fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.shadow.is_noop()
    }

    fn lower(self, _store: &RecordStore) -> ShapeRecord {
        lower::shadow(self.local_rect, self.corners, self.shadow)
    }
}
