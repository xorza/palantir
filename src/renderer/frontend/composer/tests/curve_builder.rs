//! A curve a composer test feeds, built up from the one most cases draw.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::shape::geometry::curve_basis::CurveBasis;
use glam::Vec2;

/// A cubic or an arc inside `bounds`, white, 2 px wide and unturned until a setter says otherwise,
/// at full alpha.
#[derive(Clone, Copy, Debug)]
pub(super) struct CurveBuilder {
    bounds: Rect,
    basis: CurveBasis,
    fill: GpuFill,
    width: f32,
    angle: Option<f32>,
}

impl CurveBuilder {
    pub(super) fn cubic(bounds: Rect, [p0, p1, p2, p3]: [Vec2; 4]) -> Self {
        Self::of(bounds, CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }))
    }

    pub(super) fn arc(bounds: Rect, arc: CircularArc) -> Self {
        Self::of(bounds, CurveBasis::Arc(arc))
    }

    fn of(bounds: Rect, basis: CurveBasis) -> Self {
        Self {
            bounds,
            basis,
            fill: GpuFill {
                color: RgbaF32::WHITE.to_f16(),
                ..GpuFill::default()
            },
            width: 2.0,
            angle: None,
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

    /// Turned by `angle` radians about the bounds' center, which the cull then bounds by the
    /// circle through their corners, as the encoder bounds a turned stroke.
    pub(super) const fn turned(mut self, angle: f32) -> Self {
        self.angle = Some(angle);
        self
    }

    pub(super) fn draw(self, b: &mut PaintCapture) {
        let bounds = match self.angle {
            None => StrokeBounds::Still(self.bounds),
            Some(angle) => StrokeBounds::Spun {
                spin: Spin {
                    pivot: self.bounds.center(),
                    angle,
                },
                radius: Vec2::new(self.bounds.size.w, self.bounds.size.h).length() / 2.0,
            },
        };
        b.draw(
            DrawCurvePayload {
                bounds,
                origin: Vec2::ZERO,
                basis: self.basis,
                fill: self.fill,
                width: self.width,
                ..DrawCurvePayload::default()
            },
            1.0,
        );
    }
}
