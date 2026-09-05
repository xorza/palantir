//! The drop-shadow builder. Lowers to
//! `ShapeRecord::Quad(QuadShape::Shadow)`.

use crate::primitives::corners::Corners;
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;

/// Gaussian-blurred rounded rectangle shadow.
#[derive(Clone, Debug)]
pub struct ShadowShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) corners: Corners,
    pub(crate) shadow: Shadow,
}

impl ShadowShape {
    pub(super) fn new(shadow: Shadow) -> Self {
        Self {
            local_rect: None,
            corners: Corners::ZERO,
            shadow,
        }
    }
}

impl ShadowShape {
    /// Paint into `rect`, in owner-relative coords, instead of the
    /// owner's whole arranged rect.
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn corners(mut self, corners: impl Into<Corners>) -> Self {
        self.corners = corners.into();
        self
    }
}

impl sealed::LowerShape for ShadowShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty) || self.shadow.is_noop()
    }

    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.corners.has_nan() || self.shadow.has_nan()
    }

    /// Pure repacking — the f16 lane squeeze happens in
    /// `LoweredShadow`'s `From<Shadow>`, and the paint extent is derived
    /// downstream by
    /// [`LoweredShadow::paint_rect_local`](crate::scene::shapes::paint::LoweredShadow::paint_rect_local)
    /// so damage and the encoder can't disagree about the halo. Nothing
    /// is staged, so nothing goes through `lower::`.
    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            local_rect,
            corners,
            shadow,
        } = self;
        ShapeRecord::Quad(QuadShape::Shadow {
            local_rect,
            corners,
            shadow: shadow.into(),
        })
    }
}
