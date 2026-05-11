//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use glam::{UVec2, Vec2};
use palantir::{
    Background, Button, Color, Configure, Corners, Frame, LineCap, LineJoin, Panel, Rect, Shape,
    Sizing, Stroke,
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
            .auto_id()
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
        Panel::vstack().auto_id().padding(20.0).show(ui, |ui| {
            Frame::new()
                .id_salt("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Color::rgb(0.20, 0.30, 0.55),
                    stroke: Stroke {
                        width: 2.0,
                        color: Color::rgb(0.65, 0.80, 1.00),
                    },
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
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(20.0)
            .background(Background {
                fill: pink,
                ..Default::default()
            })
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Color::TRANSPARENT,
                        stroke: Stroke {
                            width: 5.0,
                            color: Color::rgb_u8(0, 255, 0),
                        },
                        radius: Corners::new(4.0, 12.0, 20.0, 28.0),
                    })
                    .clip_rounded()
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("inner")
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
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(10.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.2, 0.3),
                        radius: Corners::all(8.0),
                        ..Default::default()
                    })
                    .clip_rounded()
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
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(0.0)
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 30.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(1.0, 0.0, 0.0),
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("cyan")
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
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("yellow")
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
                    stroke: Stroke::ZERO,
                });
            });
    });
    assert_matches_golden("interleaved_shapes_paint_order", &img, Tolerance::default());
}

/// Pin: `Shape::Line` paints a fringe-AA stroke. A diagonal 4-px
/// cyan line across a dark frame exercises the polyline cmd →
/// composer → mesh-pipeline path end-to-end. The fringe-AA fade is
/// the load-bearing visual signal — a non-AA tessellator would
/// produce a stair-stepped diagonal that fails the per-pixel
/// channel tolerance immediately.
#[test]
fn line_diagonal_aa_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(160, 120), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: Vec2::new(10.0, 10.0),
                    b: Vec2::new(150.0, 110.0),
                    width: 4.0,
                    color: Color::rgb(0.2, 0.9, 1.0),
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
                // Hairline at sub-pixel width — should appear dim
                // (alpha-faded) rather than vanish or look identical
                // to the 4 px stroke. Pins the hairline branch.
                ui.add_shape(Shape::Line {
                    a: Vec2::new(10.0, 80.0),
                    b: Vec2::new(150.0, 80.0),
                    width: 0.4,
                    color: Color::rgb(1.0, 1.0, 1.0),
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            });
    });
    assert_matches_golden("line_diagonal_aa", &img, Tolerance::default());
}

/// Pin: `Shape::Polyline` with `PolylineColors::PerPoint` paints
/// a multi-stop gradient via GPU vertex interpolation. A 4-point
/// zig-zag with four corner colors exercises the per-point
/// coloring + miter joins + composer arena copy in one frame. A
/// stride-1 inner cross-section would collapse to single-color
/// strips, which would fail the gradient sample tolerance.
#[test]
fn polyline_gradient_matches_golden() {
    use palantir::PolylineColors;
    let mut h = Harness::new();
    let img = h.render(UVec2::new(160, 140), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let pts = [
                    Vec2::new(10.0, 10.0),
                    Vec2::new(50.0, 130.0),
                    Vec2::new(90.0, 20.0),
                    Vec2::new(150.0, 130.0),
                ];
                let cols = [
                    Color::rgb(1.0, 0.2, 0.2),
                    Color::rgb(1.0, 0.85, 0.2),
                    Color::rgb(0.2, 1.0, 0.4),
                    Color::rgb(0.2, 0.6, 1.0),
                ];
                ui.add_shape(Shape::Polyline {
                    points: &pts,
                    colors: PolylineColors::PerPoint(&cols),
                    width: 5.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            });
    });
    assert_matches_golden("polyline_gradient", &img, Tolerance::default());
}

/// Pin: sharp polyline joins paint a clean bevel rather than the
/// previous miter-clamp's hard cut-off. Two strokes side by side:
/// the shallow 90° corner mitres (rendering path unchanged), the
/// tight chevron triggers the bevel-bridge codepath. Golden
/// captures both at the same width so a tessellator regression
/// (e.g. bridge winding flipped → invisible corner fill) shows up
/// as missing pixels in the right stroke only.
#[test]
fn polyline_bevel_join_matches_golden() {
    use palantir::PolylineColors;
    let mut h = Harness::new();
    let img = h.render(UVec2::new(180, 140), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let cyan = Color::rgb(0.2, 0.9, 1.0);
                let shallow = [
                    Vec2::new(15.0, 30.0),
                    Vec2::new(60.0, 60.0),
                    Vec2::new(105.0, 30.0),
                ];
                ui.add_shape(Shape::Polyline {
                    points: &shallow,
                    colors: PolylineColors::Single(cyan),
                    width: 5.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
                let sharp = [
                    Vec2::new(15.0, 100.0),
                    Vec2::new(80.0, 115.0),
                    Vec2::new(20.0, 130.0),
                ];
                ui.add_shape(Shape::Polyline {
                    points: &sharp,
                    colors: PolylineColors::Single(cyan),
                    width: 5.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            });
    });
    assert_matches_golden("polyline_bevel_join", &img, Tolerance::default());
}
