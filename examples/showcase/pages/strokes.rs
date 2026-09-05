//! Stroked geometry: widths down to sub-pixel hairlines, joins, caps,
//! per-point / per-segment polyline colours, cubic and quadratic
//! béziers, and circular arcs with solid and gradient brushes. Every
//! tile pushes raw `Shape`s through `ui.add_shape` — all of it renders
//! on the GPU curve pipeline, with no CPU tessellation anywhere.

use crate::support;
use crate::support::{demo_cell, section, tiles};
use palantir::{LineCap, LineJoin, LinearGradient, PolylineColors, RgbaF32, Shape, Stop, Ui, Vec2};

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "width — a float, not an integer; sub-pixel widths fade rather than \
         snapping to 1 px",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "widths 1–8 px", widths);
                demo_cell(ui, "hairlines 0.1–1 px", hairlines);
            });
        },
    );

    section(
        ui,
        "joins & caps — how a stroke turns, and how it ends",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "joins — Miter / Bevel / Round", joins);
                demo_cell(ui, "line caps — Butt / Square / Round", caps);
                demo_cell(ui, "curve caps — Butt / Square / Round", curve_caps);
            });
        },
    );

    section(
        ui,
        "colour — per point, per segment, or from a gradient brush",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "per-point colours", per_point);
                demo_cell(ui, "per-segment colours", per_segment);
                demo_cell(ui, "gradient along t", gradient_cubic);
                demo_cell(ui, "gradient, three stops", gradient_multistop);
            });
        },
    );

    section(ui, "curves & arcs — béziers and circular sweeps", |ui| {
        tiles(ui, |ui| {
            demo_cell(ui, "cubic bézier", cubic);
            demo_cell(ui, "quadratic bézier", quadratic);
            demo_cell(ui, "arcs & circles", arcs);
        });
    });
}

fn widths(ui: &mut Ui) {
    for (i, w) in [1.0_f32, 2.0, 3.0, 5.0, 8.0].iter().enumerate() {
        let y = 20.0 + i as f32 * 26.0;
        ui.add_shape(Shape::line(Vec2::new(16.0, y), Vec2::new(150.0, y), *w).brush(support::A));
    }
}

fn hairlines(ui: &mut Ui) {
    for (i, w) in [0.1_f32, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        let y = 20.0 + i as f32 * 26.0;
        ui.add_shape(
            Shape::line(Vec2::new(16.0, y), Vec2::new(150.0, y), *w).brush(RgbaF32::WHITE),
        );
    }
}

/// The same 90° corner three times — a non-clamp angle, so Miter really
/// mitres rather than falling back to bevel.
fn joins(ui: &mut Ui) {
    for (y, join) in [
        (22.0_f32, LineJoin::Miter),
        (66.0, LineJoin::Bevel),
        (110.0, LineJoin::Round),
    ] {
        let pts = [
            Vec2::new(24.0, y + 28.0),
            Vec2::new(84.0, y),
            Vec2::new(144.0, y + 28.0),
        ];
        ui.add_shape(Shape::polyline(&pts, PolylineColors::Single(support::A), 5.0).join(join));
    }
}

/// Three lines, one per cap style, sharing endpoints. The white marker
/// rules make the difference visible — Butt stops at the marker, Square
/// extends half a width past it, Round adds a half-disc.
fn caps(ui: &mut Ui) {
    for y in [32.0_f32, 80.0, 128.0] {
        for x in [40.0_f32, 128.0] {
            ui.add_shape(
                Shape::line(Vec2::new(x, y - 14.0), Vec2::new(x, y + 14.0), 1.0)
                    .brush(RgbaF32::WHITE),
            );
        }
    }
    for (y, color, cap) in [
        (32.0_f32, support::E, LineCap::Butt),
        (80.0, support::C, LineCap::Square),
        (128.0, support::A, LineCap::Round),
    ] {
        ui.add_shape(
            Shape::line(Vec2::new(40.0, y), Vec2::new(128.0, y), 9.0)
                .brush(color)
                .cap(cap),
        );
    }
}

/// Three identical curves, one per cap kind — the endpoint shape is the
/// only visual delta.
fn curve_caps(ui: &mut Ui) {
    for (i, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round]
        .iter()
        .enumerate()
    {
        let dy = i as f32 * 48.0;
        ui.add_shape(
            Shape::cubic_bezier(
                Vec2::new(16.0, 34.0 + dy),
                Vec2::new(50.0, 10.0 + dy),
                Vec2::new(114.0, 58.0 + dy),
                Vec2::new(150.0, 34.0 + dy),
                8.0,
            )
            .brush(support::B)
            .cap(*cap),
        );
    }
}

fn per_point(ui: &mut Ui) {
    let pts = [
        Vec2::new(16.0, 20.0),
        Vec2::new(58.0, 148.0),
        Vec2::new(104.0, 40.0),
        Vec2::new(150.0, 148.0),
    ];
    let cols = [support::E, support::B, support::C, support::A];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 4.0));
}

fn per_segment(ui: &mut Ui) {
    let pts = [
        Vec2::new(16.0, 84.0),
        Vec2::new(40.0, 34.0),
        Vec2::new(66.0, 134.0),
        Vec2::new(92.0, 34.0),
        Vec2::new(118.0, 134.0),
        Vec2::new(144.0, 34.0),
        Vec2::new(150.0, 120.0),
    ];
    let cols = [
        support::E,
        support::B,
        support::C,
        support::A,
        support::D,
        RgbaF32::hex(0xff8fc8),
    ];
    ui.add_shape(Shape::polyline(
        &pts,
        PolylineColors::PerSegment(&cols),
        4.0,
    ));
}

const P0: Vec2 = Vec2::new(16.0, 140.0);
const P1: Vec2 = Vec2::new(48.0, 20.0);
const P2: Vec2 = Vec2::new(118.0, 20.0);
const P3: Vec2 = Vec2::new(150.0, 140.0);

const Q0: Vec2 = Vec2::new(16.0, 140.0);
const Q1: Vec2 = Vec2::new(84.0, 14.0);
const Q2: Vec2 = Vec2::new(150.0, 140.0);

fn cubic(ui: &mut Ui) {
    ui.add_shape(Shape::cubic_bezier(P0, P1, P2, P3, 4.0).brush(support::A));
}

fn quadratic(ui: &mut Ui) {
    ui.add_shape(Shape::quadratic_bezier(Q0, Q1, Q2, 4.0).brush(support::C));
}

/// Two-stop gradient along the curve's t parameter (p0 → p3). The
/// `angle` field of `LinearGradient` is unused on curves.
fn gradient_cubic(ui: &mut Ui) {
    let brush = LinearGradient::two_stop(0.0, support::E, support::A);
    ui.add_shape(
        Shape::cubic_bezier(P0, P1, P2, P3, 8.0)
            .brush(brush)
            .cap(LineCap::Round),
    );
}

/// Three-stop gradient — same atlas and bake path as rounded-rect fills.
fn gradient_multistop(ui: &mut Ui) {
    let brush = LinearGradient::new(
        0.0,
        [
            Stop::new(0.0, support::E),
            Stop::new(0.5, support::B),
            Stop::new(1.0, support::A),
        ],
    );
    ui.add_shape(
        Shape::quadratic_bezier(Q0, Q1, Q2, 10.0)
            .brush(brush)
            .cap(LineCap::Round),
    );
}

fn arcs(ui: &mut Ui) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    // Full circle: a ±2π sweep closes seamlessly under Butt caps.
    ui.add_shape(Shape::circle(Vec2::new(44.0, 40.0), 28.0, 3.0).brush(support::A));
    // 3/4 sweep with a gradient along the arc (the spinner's comet
    // shape) — transparent tail to full head, round caps.
    let comet = LinearGradient::two_stop(0.0, support::B.with_alpha(0.0), support::B);
    ui.add_shape(
        Shape::arc(Vec2::new(120.0, 40.0), 28.0, -FRAC_PI_2, 1.5 * PI, 6.0)
            .brush(comet)
            .cap(LineCap::Round),
    );
    // Gauge-style bottom arc: half sweep, fat stroke, round caps.
    ui.add_shape(
        Shape::arc(Vec2::new(84.0, 118.0), 36.0, PI, PI, 10.0)
            .brush(support::C)
            .cap(LineCap::Round),
    );
    // Thin negative-sweep quarter overlaying the gauge's track.
    ui.add_shape(Shape::arc(Vec2::new(84.0, 118.0), 25.0, 0.0, -TAU * 0.25, 2.0).brush(support::E));
}
