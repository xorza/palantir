use crate::forest::element::{Configure, Element, LayoutMode};
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::shape::{LineCap, LineJoin, PolylineColors, Shape};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::palette;
use glam::Vec2;
use std::f32::consts::PI;

/// Number of samples along the arc. Enough that the round-capped
/// polyline reads as a smooth curve at typical sizes.
const SAMPLES: usize = 24;
/// Arc length in radians — a 3/4 sweep leaves a visible gap so the
/// rotation is legible.
const SWEEP: f32 = 1.5 * PI;
/// Angular velocity (radians / second).
const SPEED: f32 = 4.5;

/// Indeterminate activity spinner: a rounded arc that rotates with the
/// frame clock, its tail fading to transparent (a "comet" trail). Drives
/// its own continuous repaint while recorded, so it spins whenever it's
/// on screen and costs nothing when it isn't.
///
/// Rotation comes from [`Ui::now`] (no per-frame `dt` plumbing) and the
/// geometry is recomputed each frame — palantir has no rotation
/// transform, so the arc points are rotated directly.
pub struct Spinner {
    element: Element,
    size: f32,
    color: Option<Color>,
    thickness: Option<f32>,
}

impl Spinner {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::new(LayoutMode::Leaf),
            size: 24.0,
            color: None,
            thickness: None,
        }
    }

    /// Diameter in logical px. Default `24.0`.
    pub fn size(mut self, px: f32) -> Self {
        self.size = px;
        self
    }

    /// Arc color (head of the comet). Default the palette accent.
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// Stroke width in logical px. Default `size * 0.12` (min `1.5`).
    pub fn thickness(mut self, px: f32) -> Self {
        self.thickness = Some(px);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response<'_> {
        let size = self.size.max(1.0);
        let width = self.thickness.unwrap_or((size * 0.12).max(1.5));
        let color = self.color.unwrap_or(palette::ACCENT);
        self.element.size = (Sizing::Fixed(size), Sizing::Fixed(size)).into();

        let phase = ui.now().as_secs_f32() * SPEED;
        let id = ui.make_persistent_id(self.element.salt);
        ui.node(id, self.element, None, |ui| {
            let pts = arc_points(size, width, phase);
            let cols = comet_colors(color);
            ui.add_shape(Shape::Polyline {
                points: &pts,
                colors: PolylineColors::PerPoint(&cols),
                width,
                cap: LineCap::Round,
                join: LineJoin::Round,
            });
        });
        // Continuous animation: keep the host awake while we're on screen.
        ui.request_repaint();
        Response::lazy(id, ui)
    }
}

impl Configure for Spinner {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// Sample the arc into node-local points on a circle inset by half the
/// stroke width (so the round caps stay inside the box). The arc starts
/// at `phase` and sweeps [`SWEEP`] radians.
fn arc_points(size: f32, width: f32, phase: f32) -> [Vec2; SAMPLES] {
    let center = size * 0.5;
    let radius = (size - width).max(0.0) * 0.5;
    let mut pts = [Vec2::ZERO; SAMPLES];
    for (i, p) in pts.iter_mut().enumerate() {
        let f = i as f32 / (SAMPLES - 1) as f32;
        let a = phase + f * SWEEP;
        *p = Vec2::new(center + radius * a.cos(), center + radius * a.sin());
    }
    pts
}

/// Per-point colors for the comet trail: the tail (first point) is fully
/// transparent and the head (last point) is the full color, scaling the
/// base alpha linearly so a translucent base stays translucent.
fn comet_colors(base: Color) -> [Color; SAMPLES] {
    let mut cols = [base; SAMPLES];
    for (i, c) in cols.iter_mut().enumerate() {
        let f = i as f32 / (SAMPLES - 1) as f32;
        *c = base.with_alpha(base.a * f);
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::{SAMPLES, SWEEP, arc_points, comet_colors};
    use crate::primitives::color::Color;

    /// Every sampled point sits on the inset circle, and the arc spans
    /// exactly `SWEEP` from first to last sample. `phase` rigidly
    /// rotates the whole arc.
    #[test]
    fn arc_points_lie_on_circle_and_span_sweep() {
        let size = 24.0;
        let width = 2.0;
        let phase = 0.7;
        let pts = arc_points(size, width, phase);
        let center = size * 0.5;
        let radius = (size - width) * 0.5; // 11.0

        for p in &pts {
            let d = ((p.x - center).powi(2) + (p.y - center).powi(2)).sqrt();
            assert!(
                (d - radius).abs() < 1e-4,
                "point off circle: d={d} r={radius}"
            );
        }

        // First sample sits at `phase`, last at `phase + SWEEP`.
        let ang = |p: glam::Vec2| (p.y - center).atan2(p.x - center);
        let first = ang(pts[0]);
        assert!(
            (first - phase).abs() < 1e-4,
            "first angle {first} != phase {phase}"
        );

        // Unwrap the swept angle via the chord: total turn equals SWEEP.
        let last = pts[SAMPLES - 1];
        let expected = point_on_circle(center, radius, phase + SWEEP);
        assert!(
            (last.x - expected.x).abs() < 1e-3 && (last.y - expected.y).abs() < 1e-3,
            "last point {last:?} != swept endpoint {expected:?}",
        );
    }

    fn point_on_circle(center: f32, radius: f32, a: f32) -> glam::Vec2 {
        glam::Vec2::new(center + radius * a.cos(), center + radius * a.sin())
    }

    /// Comet trail: tail transparent, head full, monotonically rising.
    #[test]
    fn comet_colors_fade_tail_to_head() {
        let base = Color::rgb(0.6, 0.8, 1.0); // opaque
        let cols = comet_colors(base);
        assert!((cols[0].a - 0.0).abs() < 1e-6, "tail not transparent");
        assert!((cols[SAMPLES - 1].a - 1.0).abs() < 1e-6, "head not opaque");
        for w in cols.windows(2) {
            assert!(w[1].a >= w[0].a, "alpha must rise tail→head");
        }
        // RGB is untouched — only alpha varies along the trail.
        assert_eq!(cols[SAMPLES - 1].r, base.r);
        assert_eq!(cols[SAMPLES - 1].g, base.g);
        assert_eq!(cols[SAMPLES - 1].b, base.b);
    }
}
