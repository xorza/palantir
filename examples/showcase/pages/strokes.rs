//! Stroked geometry: sub-pixel hairlines to wide widths, joins, caps, per-point / per-segment polyline colours, cubic and quadratic béziers, and circular arcs in solid and ramped colours. Every tile pushes raw `Shape`s through `ui.add_shape` onto the GPU curve pipeline, with no CPU tessellation.

use crate::support;
use crate::support::{api, demo_cell, note, section, tiles};
use palantir::widget::{LineCap, LineJoin, Shape};
use palantir::{ColorRamp, RgbaF32, Stop, Stroke, Ui, Vec2};

pub(crate) fn build(ui: &mut Ui) {
    section(ui, "Width", &[api!(type Stroke)], |ui| {
        note(
            ui,
            "A float, not an integer: widths under a pixel fade instead of \
                 snapping to 1 px.",
        );
        tiles(ui, |ui| {
            demo_cell(ui, "widths 1–8 px", widths);
            demo_cell(ui, "hairlines 0.1–1 px", hairlines);
        });
    });

    section(
        ui,
        "Joins and caps",
        &[api!(type LineJoin), api!(type LineCap)],
        |ui| {
            note(
                ui,
                "How a stroke turns a corner, and how it ends. The white rules mark \
                 the line ends.",
            );
            tiles(ui, |ui| {
                demo_cell(ui, "joins — Miter / Bevel / Round", joins);
                demo_cell(ui, "line caps — Butt / Square / Round", caps);
                demo_cell(ui, "curve caps — Butt / Square / Round", curve_caps);
            });
        },
    );

    section(ui, "Colour along a stroke", &[api!(type ColorRamp)], |ui| {
        note(ui, "Per point, per segment, or a ramp along the curve.");
        tiles(ui, |ui| {
            demo_cell(ui, "per-point colours", per_point);
            demo_cell(ui, "per-segment colours", per_segment);
            demo_cell(ui, "gradient along t", gradient_cubic);
            demo_cell(ui, "gradient, three stops", gradient_multistop);
        });
    });

    section(
        ui,
        "Curves and arcs",
        &[
            api!(Shape::cubic_bezier),
            api!(Shape::quadratic_bezier),
            api!(Shape::arc),
        ],
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "cubic bézier", cubic);
                demo_cell(ui, "quadratic bézier", quadratic);
                demo_cell(ui, "arcs & circles", arcs);
            });
        },
    );
}

fn widths(ui: &mut Ui) {
    for (i, w) in [1.0_f32, 2.0, 3.0, 5.0, 8.0].iter().enumerate() {
        let y = 20.0 + i as f32 * 26.0;
        ui.add_shape(Shape::line(
            Vec2::new(16.0, y),
            Vec2::new(150.0, y),
            Stroke::new(support::A, *w),
        ));
    }
}

fn hairlines(ui: &mut Ui) {
    for (i, w) in [0.1_f32, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
        let y = 20.0 + i as f32 * 26.0;
        ui.add_shape(Shape::line(
            Vec2::new(16.0, y),
            Vec2::new(150.0, y),
            Stroke::new(RgbaF32::WHITE, *w),
        ));
    }
}

/// The same 90° corner three times; a non-clamp angle, so Miter really mitres.
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
        ui.add_shape(Shape::polyline(&pts, Stroke::new(support::A, 5.0)).join(join));
    }
}

/// Three lines, one per cap style, sharing endpoints; white marker rules show Butt stopping at the marker, Square a half width past, Round a half-disc.
fn caps(ui: &mut Ui) {
    for y in [32.0_f32, 80.0, 128.0] {
        for x in [40.0_f32, 128.0] {
            ui.add_shape(Shape::line(
                Vec2::new(x, y - 14.0),
                Vec2::new(x, y + 14.0),
                Stroke::new(RgbaF32::WHITE, 1.0),
            ));
        }
    }
    for (y, color, cap) in [
        (32.0_f32, support::E, LineCap::Butt),
        (80.0, support::C, LineCap::Square),
        (128.0, support::A, LineCap::Round),
    ] {
        ui.add_shape(
            Shape::line(
                Vec2::new(40.0, y),
                Vec2::new(128.0, y),
                Stroke::new(color, 9.0),
            )
            .cap(cap),
        );
    }
}

/// Three identical curves, one per cap kind; the endpoint shape is the only delta.
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
                Stroke::new(support::B, 8.0),
            )
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
    ui.add_shape(Shape::polyline(&pts, Stroke::new(RgbaF32::WHITE, 4.0)).per_point(&cols));
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
    ui.add_shape(Shape::polyline(&pts, Stroke::new(RgbaF32::WHITE, 4.0)).per_segment(&cols));
}

const P0: Vec2 = Vec2::new(16.0, 140.0);
const P1: Vec2 = Vec2::new(48.0, 20.0);
const P2: Vec2 = Vec2::new(118.0, 20.0);
const P3: Vec2 = Vec2::new(150.0, 140.0);

const Q0: Vec2 = Vec2::new(16.0, 140.0);
const Q1: Vec2 = Vec2::new(84.0, 14.0);
const Q2: Vec2 = Vec2::new(150.0, 140.0);

fn cubic(ui: &mut Ui) {
    ui.add_shape(Shape::cubic_bezier(
        P0,
        P1,
        P2,
        P3,
        Stroke::new(support::A, 4.0),
    ));
}

fn quadratic(ui: &mut Ui) {
    ui.add_shape(Shape::quadratic_bezier(
        Q0,
        Q1,
        Q2,
        Stroke::new(support::C, 4.0),
    ));
}

/// Two-stop ramp along the curve's t (p0 → p3) over a white stroke, so the ramp shows as authored.
fn gradient_cubic(ui: &mut Ui) {
    ui.add_shape(
        Shape::cubic_bezier(P0, P1, P2, P3, Stroke::new(RgbaF32::WHITE, 8.0))
            .ramp(ColorRamp::two_stop(support::E, support::A))
            .cap(LineCap::Round),
    );
}

/// Three-stop ramp, through the same atlas and bake path as rounded-rect fills.
fn gradient_multistop(ui: &mut Ui) {
    let ramp = ColorRamp::new([
        Stop::new(0.0, support::E),
        Stop::new(0.5, support::B),
        Stop::new(1.0, support::A),
    ]);
    ui.add_shape(
        Shape::quadratic_bezier(Q0, Q1, Q2, Stroke::new(RgbaF32::WHITE, 10.0))
            .ramp(ramp)
            .cap(LineCap::Round),
    );
}

fn arcs(ui: &mut Ui) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};
    // Full circle: a ±2π sweep closes seamlessly under Butt caps.
    ui.add_shape(Shape::circle(
        Vec2::new(44.0, 40.0),
        28.0,
        Stroke::new(support::A, 3.0),
    ));
    // 3/4 sweep with a ramp along the arc (the spinner's comet): transparent tail to full head, round caps; white ramp, stroke colour sets the hue.
    let comet = ColorRamp::two_stop(RgbaF32::WHITE.with_alpha(0.0), RgbaF32::WHITE);
    ui.add_shape(
        Shape::arc(
            Vec2::new(120.0, 40.0),
            28.0,
            -FRAC_PI_2,
            1.5 * PI,
            Stroke::new(support::B, 6.0),
        )
        .ramp(comet)
        .cap(LineCap::Round),
    );
    // Gauge-style bottom arc: half sweep, fat stroke, round caps.
    ui.add_shape(
        Shape::arc(
            Vec2::new(84.0, 118.0),
            36.0,
            PI,
            PI,
            Stroke::new(support::C, 10.0),
        )
        .cap(LineCap::Round),
    );
    // Thin negative-sweep quarter overlaying the gauge's track.
    ui.add_shape(Shape::arc(
        Vec2::new(84.0, 118.0),
        25.0,
        0.0,
        -TAU * 0.25,
        Stroke::new(support::E, 2.0),
    ));
}
