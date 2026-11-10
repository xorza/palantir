//! A rect quad a composer test feeds, built up from the one most cases draw.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint::lowered_border::LoweredBorder;
use glam::Vec2;

/// A rect quad, opaque white, sharp, strokeless and unturned until a setter says otherwise, at
/// full alpha.
#[derive(Clone, Copy, Debug)]
pub(super) struct QuadBuilder {
    rect: Rect,
    corners: Corners,
    fill: BrushSource,
    border: Border,
    spin: Option<Spin>,
}

impl QuadBuilder {
    pub(super) fn new(rect: Rect) -> Self {
        Self {
            rect,
            corners: Corners::ZERO,
            fill: BrushSource::Solid(RgbaF32::WHITE.to_f16()),
            border: Border::NONE,
            spin: None,
        }
    }

    /// Turned by `angle` radians about the rect's center.
    pub(super) fn turned(self, angle: f32) -> Self {
        let pivot = self.rect.center();
        self.turned_about(pivot, angle)
    }

    /// Turned by `angle` radians about `pivot`.
    pub(super) const fn turned_about(mut self, pivot: Vec2, angle: f32) -> Self {
        self.spin = Some(Spin { pivot, angle });
        self
    }

    pub(super) fn corners(mut self, corners: Corners) -> Self {
        self.corners = corners;
        self
    }

    pub(super) fn solid(self, color: RgbaF32) -> Self {
        self.brush(BrushSource::Solid(color.to_f16()))
    }

    pub(super) fn brush(mut self, fill: BrushSource) -> Self {
        self.fill = fill;
        self
    }

    pub(super) fn border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    pub(super) fn draw(self, buf: &mut PaintCapture) {
        buf.draw(
            DrawClosedPayload::new(
                PlacedClosed::Box(PlacedBox {
                    rect: self.rect,
                    corners: self.corners,
                }),
                ClosedFill::new(self.fill, LoweredBorder::lower(self.border)),
            )
            .turned(self.spin),
            1.0,
        );
    }
}
