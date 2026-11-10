//! A curve a composer test feeds, built up from the one most cases draw.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_stroke_payload::{DrawStrokePayload, StrokePath};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::shape::geometry::curve_basis::CurveBasis;
use glam::Vec2;

/// A cubic or an arc of an owner at `owner`, white, 2 px wide and unturned until a setter says
/// otherwise, at full alpha.
#[derive(Clone, Copy, Debug)]
pub(super) struct CurveBuilder {
    owner: Rect,
    basis: CurveBasis,
    fill: GpuFill,
    width: f32,
    angle: Option<f32>,
    origin: Vec2,
}

impl CurveBuilder {
    pub(super) fn cubic(owner: Rect, [p0, p1, p2, p3]: [Vec2; 4]) -> Self {
        Self::of(owner, CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }))
    }

    pub(super) fn arc(owner: Rect, arc: CircularArc) -> Self {
        Self::of(owner, CurveBasis::Arc(arc))
    }

    fn of(owner: Rect, basis: CurveBasis) -> Self {
        Self {
            owner,
            basis,
            fill: GpuFill {
                color: RgbaF32::WHITE.to_f16(),
                ..GpuFill::default()
            },
            width: 2.0,
            angle: None,
            origin: Vec2::ZERO,
        }
    }

    pub(super) const fn fill(mut self, fill: GpuFill) -> Self {
        self.fill = fill;
        self
    }

    pub(super) const fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Its points relative to `origin`, as the encoder hands them from the owner's position.
    pub(super) const fn origin(mut self, origin: Vec2) -> Self {
        self.origin = origin;
        self
    }

    /// Turned by `angle` radians about the owner's center, as the encoder turns a stroke.
    pub(super) const fn turned(mut self, angle: f32) -> Self {
        self.angle = Some(angle);
        self
    }

    pub(super) fn draw(self, b: &mut PaintCapture) {
        let spin = self.angle.map(|angle| Spin {
            pivot: self.owner.center(),
            angle,
        });
        b.draw(
            DrawStrokePayload {
                origin: self.origin,
                spin,
                path: StrokePath::Curve(self.basis),
                fill: self.fill,
                width: self.width,
                ..DrawStrokePayload::default()
            },
            1.0,
        );
    }
}
