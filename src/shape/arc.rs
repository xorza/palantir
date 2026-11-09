//! [`ArcShape`]: an arc of the largest circle centered in a rect.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::lower;
use crate::shape::paint::arc_geometry::ArcGeometry;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::{self, Resolve};

/// An arc of the largest circle centered in the owner's arranged rect, or [`Self::at`] a rect of
/// its own, from its start angle turning by its sweep, as Compose's `drawArc` runs on its rect's
/// oval. A [`Stroke`] draws it, centered on the circle.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct ArcShape {
    pub(crate) local_rect: Option<Rect>,
    pub(crate) start_angle: f32,
    /// Coerced to one full circle, `±2π`, its sign kept: a longer sweep traces the same circle.
    pub(crate) sweep: f32,
}

impl ArcShape {
    pub(super) const fn new(start_angle: f32, sweep: f32) -> Self {
        Self {
            local_rect: None,
            start_angle,
            sweep: sweep.clamp(-std::f32::consts::TAU, std::f32::consts::TAU),
        }
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

impl sealed::LowerShape for (ArcShape, Stroke) {
    #[track_caller]
    fn validate(&self) {
        self.1.validate();
    }

    fn lower(self, _resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            ArcShape {
                local_rect,
                start_angle,
                sweep,
            },
            stroke,
        ) = self;
        let arc = ArcGeometry {
            local_rect,
            start_angle,
            sweep,
        };
        lower::arc(store, arc, stroke)
    }
}
