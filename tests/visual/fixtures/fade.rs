//! Colours that fade to transparent, interpolated premultiplied.

use glam::{UVec2, Vec2};
use palantir::widget::{Mesh, Shape};
use palantir::{LinearGradient, Rect, RgbaF32, Stroke};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px, canvas};
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
            canvas(ui, |ui| {
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
    // Premultiplied white at alpha 1 − 99.5 / 200 = 0.5025, over black.
    let midway = RgbaF32::new(0.5025, 0.5025, 0.5025, 1.0).to_srgba_u8();
    for (label, y) in [("mesh", 20), ("polyline", 60), ("gradient", 100)] {
        assert_px(
            img.get_pixel(99, y).0,
            [midway.r, midway.g, midway.b, 255],
            SRGB_ROUND_TRIP,
            format_args!("{label}: premultiplied white at half alpha"),
        );
    }
}
