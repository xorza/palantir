//! Line and curve strokes: widths down to sub-pixel hairlines, joins,
//! caps, per-point / per-segment polyline colors, cubic / quadratic
//! béziers, and circular arcs with solid and gradient brushes. Every
//! cell paints raw `Shape`s via `ui.add_shape` into a captioned demo
//! surface.

use crate::support;
use crate::support::{cell_row, demo_cell};
use aperture::{Brush, Color, LineCap, LineJoin, LinearGradient, PolylineColors, Shape, Stop, Ui};
use glam::Vec2;

pub(crate) fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        cell_row(ui, "row1", |ui| {
            demo_cell(ui, "widths 1–8 px", widths);
            demo_cell(ui, "hairlines 0.1–1 px", hairlines);
            demo_cell(ui, "joins — Miter / Bevel / Round", joins);
            demo_cell(ui, "caps — Butt / Square / Round", caps);
        });
        cell_row(ui, "row2", |ui| {
            demo_cell(ui, "per-point colors", gradient);
            demo_cell(ui, "per-segment colors", per_segment);
            demo_cell(ui, "cubic bézier", cubic);
            demo_cell(ui, "quadratic bézier", quadratic);
        });
        cell_row(ui, "row3", |ui| {
            demo_cell(ui, "gradient cubic (along t)", gradient_cubic);
            demo_cell(ui, "gradient multi-stop", gradient_multistop);
            demo_cell(ui, "curve caps — Butt / Square / Round", curve_caps);
            demo_cell(ui, "arcs & circles", arcs);
        });
    });
}

fn widths(ui: &mut Ui) {
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    for (i, w) in [1.0_f32, 2.0, 3.0, 5.0, 8.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::line(Vec2::new(10.0, y), Vec2::new(110.0, y), *w).brush(cyan));
    }
}

fn hairlines(ui: &mut Ui) {
    let white = Color::rgb(1.0, 1.0, 1.0);
    for (i, w) in [0.1_f32, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        let y = 12.0 + i as f32 * 20.0;
        ui.add_shape(Shape::line(Vec2::new(10.0, y), Vec2::new(110.0, y), *w).brush(white));
    }
}

fn joins(ui: &mut Ui) {
    // Same 90° corner painted three times to highlight join styles
    // at a non-clamp angle (where Miter actually mitres rather
    // than falling back to bevel). Row 1: Miter (sharp point).
    // Row 2: Bevel (flat cut). Row 3: Round (curved arc).
    let cyan = Color::rgb(0.2, 0.9, 1.0);
    for (y, join) in [
        (15.0_f32, LineJoin::Miter),
        (50.0, LineJoin::Bevel),
        (85.0, LineJoin::Round),
    ] {
        let pts = [
            Vec2::new(15.0, y + 25.0),
            Vec2::new(60.0, y),
            Vec2::new(105.0, y + 25.0),
        ];
        ui.add_shape(Shape::polyline(&pts, PolylineColors::Single(cyan), 5.0).join(join));
    }
}

fn caps(ui: &mut Ui) {
    // Three lines, one per cap style. All share the same endpoints; the
    // marker rules at the ends make the difference visible — Butt stops
    // at the marker, Square extends by half-width past it, Round adds a
    // half-disc.
    let red = Color::rgb(1.0, 0.4, 0.4);
    let green = Color::rgb(0.4, 1.0, 0.4);
    let blue = Color::rgb(0.4, 0.6, 1.0);
    let marker = Color::rgb(1.0, 1.0, 1.0);
    for y in [25.0_f32, 60.0, 95.0] {
        for x in [30.0_f32, 90.0] {
            ui.add_shape(
                Shape::line(Vec2::new(x, y - 12.0), Vec2::new(x, y + 12.0), 1.0).brush(marker),
            );
        }
    }
    for (y, color, cap) in [
        (25.0_f32, red, LineCap::Butt),
        (60.0, green, LineCap::Square),
        (95.0, blue, LineCap::Round),
    ] {
        ui.add_shape(
            Shape::line(Vec2::new(30.0, y), Vec2::new(90.0, y), 8.0)
                .brush(color)
                .cap(cap),
        );
    }
}

fn gradient(ui: &mut Ui) {
    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(40.0, 110.0),
        Vec2::new(70.0, 30.0),
        Vec2::new(110.0, 110.0),
    ];
    let cols = [
        Color::rgb(1.0, 0.2, 0.2),
        Color::rgb(1.0, 0.85, 0.2),
        Color::rgb(0.2, 1.0, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
    ];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 4.0));
}

fn per_segment(ui: &mut Ui) {
    let pts = [
        Vec2::new(10.0, 60.0),
        Vec2::new(30.0, 30.0),
        Vec2::new(50.0, 90.0),
        Vec2::new(70.0, 30.0),
        Vec2::new(90.0, 90.0),
        Vec2::new(110.0, 30.0),
        Vec2::new(110.0, 90.0),
    ];
    let cols = [
        Color::rgb(1.0, 0.2, 0.2),
        Color::rgb(1.0, 0.85, 0.2),
        Color::rgb(0.2, 1.0, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
        Color::rgb(0.7, 0.3, 1.0),
        Color::rgb(1.0, 0.5, 0.8),
    ];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::PerSegment(&cols),
        4.0,
    ));
}

const P0: Vec2 = Vec2::new(10.0, 100.0);
const P1: Vec2 = Vec2::new(35.0, 10.0);
const P2: Vec2 = Vec2::new(85.0, 10.0);
const P3: Vec2 = Vec2::new(110.0, 100.0);

const Q0: Vec2 = Vec2::new(10.0, 100.0);
const Q1: Vec2 = Vec2::new(60.0, 5.0);
const Q2: Vec2 = Vec2::new(110.0, 100.0);

fn cubic(ui: &mut Ui) {
    ui.add_shape(Shape::cubic_bezier(P0, P1, P2, P3, 4.0).brush(Color::rgb(0.2, 0.9, 1.0)));
}

fn quadratic(ui: &mut Ui) {
    ui.add_shape(Shape::quadratic_bezier(Q0, Q1, Q2, 4.0).brush(Color::rgb(0.4, 1.0, 0.5)));
}

fn gradient_cubic(ui: &mut Ui) {
    // Two-stop gradient along the curve's t parameter (p0 → p3). The
    // `angle` field of LinearGradient is unused on curves.
    let brush = Brush::Linear(LinearGradient::two_stop(
        0.0,
        Color::rgb(1.0, 0.2, 0.4),
        Color::rgb(0.2, 0.6, 1.0),
    ));
    ui.add_shape(
        Shape::cubic_bezier(P0, P1, P2, P3, 8.0)
            .brush(brush)
            .cap(LineCap::Round),
    );
}

fn gradient_multistop(ui: &mut Ui) {
    // Three-stop rainbow gradient. Same atlas + bake path as
    // RoundedRect linear fills.
    let brush = Brush::Linear(LinearGradient::new(
        0.0,
        [
            Stop::new(0.0, Color::rgb(1.0, 0.2, 0.2)),
            Stop::new(0.5, Color::rgb(1.0, 0.9, 0.2)),
            Stop::new(1.0, Color::rgb(0.2, 0.6, 1.0)),
        ],
    ));
    ui.add_shape(
        Shape::quadratic_bezier(Q0, Q1, Q2, 10.0)
            .brush(brush)
            .cap(LineCap::Round),
    );
}

fn arcs(ui: &mut Ui) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    // Full circle: a ±2π sweep closes seamlessly under Butt caps.
    ui.add_shape(Shape::circle(Vec2::new(35.0, 35.0), 25.0, 3.0).brush(Color::rgb(0.2, 0.9, 1.0)));
    // 3/4 sweep with a gradient along the arc (the spinner's comet
    // shape) — transparent tail to full head, round caps.
    let comet = Brush::Linear(LinearGradient::two_stop(
        0.0,
        Color::rgb(1.0, 0.85, 0.2).with_alpha(0.0),
        Color::rgb(1.0, 0.85, 0.2),
    ));
    ui.add_shape(
        Shape::arc(Vec2::new(85.0, 35.0), 25.0, -FRAC_PI_2, 1.5 * PI, 6.0)
            .brush(comet)
            .cap(LineCap::Round),
    );
    // Gauge-style bottom arc: half sweep, fat stroke, round caps.
    ui.add_shape(
        Shape::arc(Vec2::new(60.0, 125.0), 40.0, PI, PI, 10.0)
            .brush(Color::rgb(0.4, 1.0, 0.5))
            .cap(LineCap::Round),
    );
    // Thin negative-sweep quarter overlaying the gauge's track.
    ui.add_shape(
        Shape::arc(Vec2::new(60.0, 125.0), 28.0, 0.0, -TAU * 0.25, 2.0)
            .brush(Color::rgb(1.0, 0.4, 0.4)),
    );
}

fn curve_caps(ui: &mut Ui) {
    // Three identical curves, one per cap kind — the endpoint shape
    // is the only visual delta. Mirrors `curve_caps_match_golden`.
    for (i, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round]
        .iter()
        .enumerate()
    {
        let dy = i as f32 * 35.0;
        ui.add_shape(
            Shape::cubic_bezier(
                Vec2::new(10.0, 25.0 + dy),
                Vec2::new(35.0, 5.0 + dy),
                Vec2::new(85.0, 45.0 + dy),
                Vec2::new(110.0, 25.0 + dy),
                8.0,
            )
            .brush(Color::rgb(1.0, 0.85, 0.2))
            .cap(*cap),
        );
    }
}
