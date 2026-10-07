//! A rect quad a composer test feeds, built up from the one most cases draw.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;

/// A rect quad, opaque white, sharp and strokeless until a setter says otherwise, at full alpha.
#[derive(Debug)]
pub(super) struct QuadBuilder {
    rect: Rect,
    corners: Corners,
    fill: BrushSource,
    stroke: Stroke,
}

impl QuadBuilder {
    pub(super) fn new(rect: Rect) -> Self {
        Self {
            rect,
            corners: Corners::ZERO,
            fill: BrushSource::Solid(RgbaF32::WHITE.into()),
            stroke: Stroke::NONE,
        }
    }

    pub(super) fn corners(mut self, corners: Corners) -> Self {
        self.corners = corners;
        self
    }

    pub(super) fn solid(self, color: RgbaF32) -> Self {
        self.brush(BrushSource::Solid(color.into()))
    }

    pub(super) fn brush(mut self, fill: BrushSource) -> Self {
        self.fill = fill;
        self
    }

    pub(super) fn stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = stroke;
        self
    }

    pub(super) fn draw(self, buf: &mut PaintCapture) {
        buf.draw_quad(
            DrawQuadPayload::rect(self.rect, self.corners, self.fill, self.stroke.into()),
            1.0,
        );
    }
}
