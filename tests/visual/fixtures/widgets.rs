//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use glam::UVec2;
use palantir::{
    Background, Button, Color, Configure, Corners, Frame, Panel, Rect, Shape, Sizing, Stroke,
    Surface,
};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

#[test]
fn button_hello_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(256, 96), 1.0, DARK_BG, |ui| {
        Button::new()
            .label("hello")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui);
    });
    assert_matches_golden("button_hello", &img, Tolerance::default());
}

/// Exercises the rounded-rect SDF AA path: solid fill, visible stroke,
/// non-trivial corner radius, padded inside a darker scene.
#[test]
fn frame_filled_with_stroke_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().padding(20.0).show(ui, |ui| {
            Frame::new()
                .with_id("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.20, 0.30, 0.55),
                    stroke: Some(Stroke {
                        width: 2.0,
                        color: Color::rgb(0.65, 0.80, 1.00),
                    }),
                    radius: Corners::all(16.0),
                })
                .show(ui);
        });
    });
    assert_matches_golden("frame_filled_with_stroke", &img, Tolerance::default());
}

/// Pins the rounded-clip stencil path. Layered: full-canvas pink, then
/// a smaller rounded panel (per-corner distinct radii, 1px black
/// stroke, rounded clip), then a full-fill black child whose square
/// corners must be trimmed by the stencil mask. Per-corner radii test
/// the SDF's corner mixing — uniform-radius bug would still pass a
/// `Corners::all(...)` fixture.
#[test]
fn surface_rounded_clips_full_fill_child() {
    let mut h = Harness::new();
    let pink = Color::rgb(1.0, 0.42, 0.72);
    let black = Color::rgb(0.0, 0.0, 0.0);
    let img = h.render(UVec2::new(220, 220), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(20.0)
            .background(Background {
                fill: pink,
                ..Default::default()
            })
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Surface::clip_rounded_with_bg(Background {
                        fill: Color::TRANSPARENT,
                        stroke: Some(Stroke {
                            width: 5.0,
                            color: Color::rgb_u8(0, 255, 0),
                        }),
                        radius: Corners::new(4.0, 12.0, 20.0, 28.0),
                    }))
                    .show(ui, |ui| {
                        Frame::new()
                            .with_id("inner")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: black,
                                ..Default::default()
                            })
                            .show(ui);
                    });
            });
    });
    assert_matches_golden(
        "surface_rounded_clips_full_fill_child",
        &img,
        Tolerance::default(),
    );
}

/// Pin the backbuffer-rebuild invariant: when the surface texture
/// changes size between rounded-clip frames, `WgpuBackend` must
/// reset its stencil attachment along with the color backbuffer. If
/// the old stencil leaks across the resize, wgpu validation panics
/// because the stencil texture's size no longer matches the render
/// pass attachment. Smoke test: two rounded-clip renders at
/// different sizes, no golden assertion — surviving the second
/// `submit` without panic is the assertion.
#[test]
fn rounded_clip_survives_surface_resize() {
    let mut h = Harness::new();
    let scene = |ui: &mut palantir::Ui| {
        Panel::vstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(10.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .with_id("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Surface::clip_rounded_with_bg(Background {
                        fill: Color::rgb(0.2, 0.2, 0.3),
                        radius: Corners::all(8.0),
                        ..Default::default()
                    }))
                    .show(ui, |_| {});
            });
    };
    let _ = h.render(UVec2::new(120, 120), 1.0, DARK_BG, scene);
    let _ = h.render(UVec2::new(240, 200), 1.0, DARK_BG, scene);
    // If `ensure_backbuffer` failed to reset `bb.stencil = None`, the
    // second render would attach a 120×120 stencil to a 240×200 pass
    // and wgpu validation would have already panicked above.
}

/// Pin the slot mechanism end-to-end: a parent records three sub-rect
/// shapes interleaved with two child Frame nodes. Each shape's rect
/// **overlaps the children that should paint underneath it**, so the
/// final pixels distinguish "shape painted at the right slot" from
/// "all shapes collapsed to slot 0".
///
/// Layout (220×60 hstack, no padding, no gap):
/// - red sub-rect at x=0..30 (slot 0, hidden by cyan child).
/// - cyan child at x=0..60.
/// - green sub-rect at x=30..90 (slot 1: covers cyan's right half;
///   yellow then paints over green's right half).
/// - yellow child at x=60..120.
/// - blue sub-rect at x=90..150 (slot 2: covers yellow's right half
///   + extends past it).
///
/// Expected pixels: cyan(0..30), green(30..60), yellow(60..90),
/// blue(90..150). If slots collapsed to 0, the visible order would
/// instead be cyan(0..60), yellow(60..120), blue(120..150).
#[test]
fn interleaved_shapes_paint_in_record_order() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 60), 1.0, DARK_BG, |ui| {
        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(0.0)
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 30.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(1.0, 0.0, 0.0),
                    stroke: None,
                });
                Frame::new()
                    .with_id("cyan")
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 1.0),
                        ..Default::default()
                    })
                    .size((Sizing::Fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(30.0, 0.0, 60.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(0.0, 1.0, 0.0),
                    stroke: None,
                });
                Frame::new()
                    .with_id("yellow")
                    .background(Background {
                        fill: Color::rgb(1.0, 1.0, 0.0),
                        ..Default::default()
                    })
                    .size((Sizing::Fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(90.0, 0.0, 60.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(0.2, 0.4, 1.0),
                    stroke: None,
                });
            });
    });
    assert_matches_golden("interleaved_shapes_paint_order", &img, Tolerance::default());
}
