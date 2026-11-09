//! [`CircleShape`]: the largest circle centered in a rect.

use std::f32::consts::TAU;

use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower;
use crate::shape::paint::arc_geometry::ArcGeometry;
use crate::shape::paint::quad_shape::{BoxGeometry, BoxOutline};
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::{self, IntoPaint, Resolve};

/// The largest circle centered in the owner's arranged rect, or [`Self::at`] a rect of its own,
/// as SwiftUI's `Circle` fits the rect it is offered and WPF's `Ellipse` fills its slot. A
/// [`Paint`](crate::Paint) covers it as a round box, its border inside its edge; a [`Stroke`] draws it as a
/// closed arc, centered on the circle.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct CircleShape {
    pub(crate) local_rect: Option<Rect>,
}

impl CircleShape {
    pub(super) const fn new() -> Self {
        Self { local_rect: None }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }
}

/// A round box of `P`.
impl<P: IntoPaint> sealed::LowerShape for (CircleShape, P) {
    const CAN_BE_PANE: bool = true;

    #[track_caller]
    fn validate(&self) {
        self.1.clone().into().validate();
    }

    fn lower(self, resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (CircleShape { local_rect }, paint) = self;
        let geometry = BoxGeometry {
            local_rect,
            outline: BoxOutline::Circle,
            coverage: Coverage::Inside,
        };
        lower::paint_box(resolve, store, geometry, &paint.into())
    }
}

/// A closed arc drawn with a [`Stroke`].
impl sealed::LowerShape for (CircleShape, Stroke) {
    #[track_caller]
    fn validate(&self) {
        self.1.validate();
    }

    fn lower(self, _resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (CircleShape { local_rect }, stroke) = self;
        let arc = ArcGeometry {
            local_rect,
            start_angle: 0.0,
            sweep: TAU,
        };
        lower::arc(store, arc, stroke)
    }
}
