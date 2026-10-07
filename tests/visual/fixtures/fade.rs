//! Colours that fade to transparent, interpolated premultiplied.

use glam::{UVec2, Vec2};
use palantir::widget::{Mesh, Shape};
use palantir::{Configure, LinearGradient, Panel, Rect, RgbaF32, Sizing, Stroke};

use crate::harness::Harness;

/// White fading to transparent black across 200 px over black, on every colour-interpolating path (mesh vertices, polyline points, gradient stops). Premultiplied, the midpoint is white at half alpha (0.5 linear, sRGB 188); straight, it was half-grey (0.25 linear, sRGB 137). Pixel 99 sits at t = 99.5 / 200.
#[test]
fn a_fade_to_transparent_keeps_its_colour_midway() {
    let mut mesh = Mesh::new();
    let (white, clear) = (RgbaF32::WHITE, RgbaF32::TRANSPARENT);
    let v = [
        mesh.vertex(Vec2::new(0.0, 10.0), white),
        mesh.vertex(Vec2::new(200.0, 10.0), clear),
        mesh.vertex(Vec2::new(0.0, 30.0), white),
        mesh.vertex(Vec2::new(200.0, 30.0), clear),
    ];
    mesh.triangle(v[0], v[1], v[2]);
    mesh.triangle(v[1], v[3], v[2]);
    let line = [Vec2::new(0.0, 60.0), Vec2::new(200.0, 60.0)];

    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(200, 120))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::canvas()
                .id_salt("fades")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::mesh(&mesh));
                    ui.add_shape(
                        Shape::polyline(&line, Stroke::new(white, 20.0)).per_point(&[white, clear]),
                    );
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 90.0, 200.0, 20.0))
                            .fill(LinearGradient::two_stop(0.0, white, clear)),
                    );
                });
        })
        .image;
    for (label, y) in [("mesh", 20), ("polyline", 60), ("gradient", 100)] {
        let [r, g, b, _] = img.get_pixel(99, y).0;
        assert!(
            (180..=195).contains(&r) && r == g && g == b,
            "{label}: midpoint {r}, {g}, {b}; premultiplied white at half alpha is 188",
        );
    }
}
