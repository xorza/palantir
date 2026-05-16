//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use glam::{UVec2, Vec2};
use palantir::{
    Background, Brush, Button, Color, ColorU8, Configure, ConicGradient, Corners, Frame, LineCap,
    LineJoin, LinearGradient, Panel, RadialGradient, Rect, Shadow, Shape, Sizing, Stroke,
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
                    fill: Color::rgb(0.20, 0.30, 0.55).into(),
                    stroke: Stroke::solid(Color::rgb(0.65, 0.80, 1.00), 2.0),
                    radius: Corners::all(16.0),
                    shadow: Shadow::NONE,
                })
                .show(ui);
        });
    });
    assert_matches_golden("frame_filled_with_stroke", &img, Tolerance::default());
}

/// Pin the linear-gradient paint path end-to-end: composer registers
/// the gradient with the LUT atlas, backend uploads the row, shader
/// samples the LUT in the brush-slot branch. A vertical (π/2 angle)
/// 2-stop gradient from a dark-navy to a brighter-blue gives a clear
/// luminance ramp that's eyeballable in the golden and catches both
/// the wiring and the shader sample position.
#[test]
fn frame_linear_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().auto_id().padding(20.0).show(ui, |ui| {
            Frame::new()
                .id_salt("card")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        std::f32::consts::FRAC_PI_2,
                        ColorU8::hex(0x1a1a2e),
                        ColorU8::hex(0x4c5cdb),
                    )),
                    radius: Corners::all(16.0),
                    ..Default::default()
                })
                .show(ui);
        });
    });
    assert_matches_golden("frame_linear_gradient", &img, Tolerance::default());
}

/// Pin: `Shape::RoundedRect { fill: Brush::Linear(...) }` lowered
/// through `Tree::add_shape` → `ShapeRecord::RoundedRect { fill:
/// Brush, .. }` paints correctly. Slice-2 step 6 unblocks this — prior
/// to the widening, the lowering called `as_solid().expect(...)` and
/// panicked on any non-solid brush.
#[test]
fn add_shape_rounded_rect_linear_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack().auto_id().padding(20.0).show(ui, |ui| {
            ui.add_shape(Shape::RoundedRect {
                local_rect: Some(Rect::new(0.0, 0.0, 180.0, 100.0)),
                radius: Corners::all(12.0),
                fill: Brush::Linear(LinearGradient::two_stop(
                    0.0,
                    ColorU8::hex(0xff5e44),
                    ColorU8::hex(0xfacc15),
                )),
                stroke: Stroke::ZERO,
            });
        });
    });
    assert_matches_golden(
        "add_shape_rounded_rect_linear_gradient",
        &img,
        Tolerance::default(),
    );
}

/// Render the showcase's gradients tab as a single golden so the
/// six demo cells are pinned end-to-end: two-stop horizontal /
/// vertical / 45°, three-stop, three spread modes stacked, three
/// interp spaces stacked. Acts as an eyeball-replacement for the
/// "open the showcase" check the slice-2 plan asked for, and locks
/// the visual against shader / atlas drift.
#[test]
fn showcase_gradients_tab_matches_golden() {
    use palantir::{Interp, Spread, Stop};
    let mut h = Harness::new();
    let img = h.render(UVec2::new(560, 360), 1.0, DARK_BG, |ui| {
        let navy = ColorU8::hex(0x1a1a2e);
        let blue = ColorU8::hex(0x4c5cdb);
        let orange = ColorU8::hex(0xff7e44);
        let yellow = ColorU8::hex(0xfacc15);
        let red = ColorU8::hex(0xff5e44);
        let green = ColorU8::hex(0x46c46c);
        let cell = |g: LinearGradient| Background {
            fill: Brush::Linear(g),
            radius: Corners::all(8.0),
            ..Default::default()
        };
        Panel::vstack()
            .auto_id()
            .gap(16.0)
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::hstack()
                    .id_salt("row1")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("horizontal")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(cell(LinearGradient::two_stop(0.0, navy, blue)))
                            .show(ui);
                        Frame::new()
                            .id_salt("vertical")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(cell(LinearGradient::two_stop(
                                std::f32::consts::FRAC_PI_2,
                                navy,
                                blue,
                            )))
                            .show(ui);
                        Frame::new()
                            .id_salt("diag")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(cell(LinearGradient::two_stop(
                                std::f32::consts::FRAC_PI_4,
                                orange,
                                yellow,
                            )))
                            .show(ui);
                    });
                Panel::hstack()
                    .id_salt("row2")
                    .gap(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("threestop")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(cell(LinearGradient::three_stop(0.0, red, yellow, green)))
                            .show(ui);
                        Panel::vstack()
                            .id_salt("spread")
                            .gap(4.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for (i, sp) in [Spread::Pad, Spread::Repeat, Spread::Reflect]
                                    .iter()
                                    .enumerate()
                                {
                                    let g = LinearGradient::new(
                                        0.0,
                                        [Stop::new(0.0, navy), Stop::new(0.5, blue)],
                                    )
                                    .with_spread(*sp);
                                    Frame::new()
                                        .id_salt(("sp", i))
                                        .size((Sizing::FILL, Sizing::FILL))
                                        .background(cell(g))
                                        .show(ui);
                                }
                            });
                        Panel::vstack()
                            .id_salt("interp")
                            .gap(4.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                for (i, ip) in [Interp::Linear, Interp::Oklab].iter().enumerate() {
                                    let g =
                                        LinearGradient::two_stop(0.0, red, green).with_interp(*ip);
                                    Frame::new()
                                        .id_salt(("ip", i))
                                        .size((Sizing::FILL, Sizing::FILL))
                                        .background(cell(g))
                                        .show(ui);
                                }
                            });
                    });
            });
    });
    assert_matches_golden("showcase_gradients_tab", &img, Tolerance::default());
}

/// Pins the radial + conic shader paths end-to-end. Two side-by-side
/// frames: a centred radial (yellow core fading to navy) and a 4-stop
/// conic colour wheel. Mismatch flags drift in `eval_fill`'s radial /
/// conic branches, the atlas (stops, interp) keying, or the
/// `fill_axis` payload packing.
#[test]
fn radial_and_conic_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(320, 160), 1.0, DARK_BG, |ui| {
        Panel::hstack()
            .auto_id()
            .gap(16.0)
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let r = RadialGradient::two_stop_centered(
                    ColorU8::hex(0xfacc15),
                    ColorU8::hex(0x1a1a2e),
                );
                Frame::new()
                    .id_salt("radial")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Brush::Radial(r),
                        radius: Corners::all(8.0),
                        ..Default::default()
                    })
                    .show(ui);
                let c = ConicGradient::new(
                    glam::Vec2::splat(0.5),
                    0.0,
                    [
                        palantir::Stop::new(0.0, ColorU8::hex(0xff5e44)),
                        palantir::Stop::new(0.25, ColorU8::hex(0xfacc15)),
                        palantir::Stop::new(0.5, ColorU8::hex(0x46c46c)),
                        palantir::Stop::new(0.75, ColorU8::hex(0x4c5cdb)),
                        palantir::Stop::new(1.0, ColorU8::hex(0xff5e44)),
                    ],
                );
                Frame::new()
                    .id_salt("conic")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Brush::Conic(c),
                        radius: Corners::all(8.0),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });
    assert_matches_golden("radial_and_conic_gradient", &img, Tolerance::default());
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
                fill: pink.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Color::TRANSPARENT.into(),
                        stroke: Stroke::solid(Color::rgb_u8(0, 255, 0), 5.0),
                        radius: Corners::new(4.0, 12.0, 20.0, 28.0),
                        shadow: Shadow::NONE,
                    })
                    .clip_rounded()
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("inner")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: black.into(),
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

/// Regression: rounded clip whose rect extends off-screen on every
/// side. The mask SDF must use the panel's true rect — not the
/// viewport-clamped scissor — so the rounded corners stay outside
/// the viewport instead of "sliding inward" into the visible region.
/// Visible: a black panel with a green stroke filling the entire
/// viewport with NO rounded corners visible (they sit off-screen);
/// without the fix, dark curved notches appear at each viewport
/// corner where the stencil mask cuts the rect.
#[test]
fn rounded_clip_partially_offscreen_does_not_bleed_corners() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(120, 90), 1.0, DARK_BG, |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .position(Vec2::new(-40.0, -30.0))
                    .size((Sizing::Fixed(200.0), Sizing::Fixed(150.0)))
                    .background(Background {
                        fill: Color::TRANSPARENT.into(),
                        stroke: Stroke::solid(Color::rgb_u8(0, 255, 0), 4.0),
                        radius: Corners::all(24.0),
                        shadow: Shadow::NONE,
                    })
                    .clip_rounded()
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt("inner")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::rgb(0.0, 0.0, 0.0).into(),
                                ..Default::default()
                            })
                            .show(ui);
                    });
            });
    });
    assert_matches_golden(
        "rounded_clip_partially_offscreen",
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
    let scene = |ui: &mut palantir::UiCore| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(10.0)
            .show(ui, |ui| {
                Panel::zstack()
                    .id_salt("rounded")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.2, 0.3).into(),
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
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("cyan")
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 1.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::Fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(30.0, 0.0, 60.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(0.0, 1.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("yellow")
                    .background(Background {
                        fill: Color::rgb(1.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::Fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(90.0, 0.0, 60.0, 60.0)),
                    radius: Corners::default(),
                    fill: Color::rgb(0.2, 0.4, 1.0).into(),
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
                    brush: Color::rgb(0.2, 0.9, 1.0).into(),
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
                    brush: Color::rgb(1.0, 1.0, 1.0).into(),
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

/// Pin: `LineCap::Round` paints a half-disc fan at each endpoint
/// — visible as the rounded ends of a thick stroke. Golden also
/// compares Butt + Square + Round side by side: cap-style
/// regressions (e.g. Round collapsing to Butt) show up as missing
/// arc pixels at the end of the bottom stroke.
#[test]
fn polyline_round_caps_match_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(180, 140), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (y, cap, color) in [
                    (30.0_f32, LineCap::Butt, Color::rgb(1.0, 0.4, 0.4)),
                    (70.0, LineCap::Square, Color::rgb(0.4, 1.0, 0.4)),
                    (110.0, LineCap::Round, Color::rgb(0.4, 0.6, 1.0)),
                ] {
                    ui.add_shape(Shape::Line {
                        a: Vec2::new(40.0, y),
                        b: Vec2::new(140.0, y),
                        width: 10.0,
                        brush: color.into(),
                        cap,
                        join: LineJoin::Miter,
                    });
                }
            });
    });
    assert_matches_golden("polyline_round_caps", &img, Tolerance::default());
}

/// Pin: `LineJoin::Round` paints a curved arc at interior joins.
/// Three identical 90° corners with Miter / Bevel / Round joins
/// — Miter shows a sharp point, Bevel a flat cut, Round a smooth
/// arc. Visually distinct golden ensures the join-style branch
/// reaches the tessellator and emits the right geometry.
#[test]
fn polyline_round_join_matches_golden() {
    use palantir::PolylineColors;
    let mut h = Harness::new();
    let img = h.render(UVec2::new(180, 200), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let cyan = Color::rgb(0.2, 0.9, 1.0);
                for (y, join) in [
                    (30.0_f32, LineJoin::Miter),
                    (90.0, LineJoin::Bevel),
                    (150.0, LineJoin::Round),
                ] {
                    let pts = [
                        Vec2::new(20.0, y + 40.0),
                        Vec2::new(90.0, y),
                        Vec2::new(160.0, y + 40.0),
                    ];
                    ui.add_shape(Shape::Polyline {
                        points: &pts,
                        colors: PolylineColors::Single(cyan),
                        width: 8.0,
                        cap: LineCap::Butt,
                        join,
                    });
                }
            });
    });
    assert_matches_golden("polyline_round_join", &img, Tolerance::default());
}

/// Pin: a translucent polyline must blend through
/// `PREMULTIPLIED_ALPHA_BLENDING` correctly — the mesh pipeline's
/// fragment shader must premultiply its straight-alpha vertex tint
/// at output. The visual test paints a translucent green stroke
/// (linear `(0, 1, 0)`, α=0.5) over an opaque magenta backdrop
/// (linear `(1, 0, 1)`).
///
/// Correct premul source: linear blend yields `(0.5, 0.5, 0.5)` →
/// sRGB-encoded ~`(188, 188, 188)` mid-grey.
/// Bug (straight-alpha source mis-routed into premul blend): linear
/// blend yields `(0.5, 1, 0.5)` → sRGB-encoded ~`(188, 255, 188)` —
/// bright green tint, green channel >220.
///
/// Test asserts `green - max(red, blue) < 32` at the polyline's
/// center pixel. A regression of the `mesh.wgsl::fs` premultiply
/// step fails this with `delta ≈ 60+`.
#[test]
fn polyline_translucent_premultiplies_in_mesh_shader() {
    use palantir::PolylineColors;
    let mut h = Harness::new();
    // Backdrop + a 24px horizontal translucent green stroke at y=60.
    let img = h.render(UVec2::new(120, 120), 1.0, Color::BLACK, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 120.0, 120.0)),
                    radius: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
                let pts = [Vec2::new(10.0, 60.0), Vec2::new(110.0, 60.0)];
                ui.add_shape(Shape::Polyline {
                    points: &pts,
                    colors: PolylineColors::Single(Color::rgba(0.0, 1.0, 0.0, 0.5)),
                    width: 24.0,
                    cap: LineCap::Butt,
                    join: LineJoin::Miter,
                });
            });
    });
    // Sample the stroke's center (x=60, y=60). RgbaImage is
    // sRGB-encoded after the swapchain target's auto-encode.
    let px = img.get_pixel(60, 60);
    let r = px.0[0] as i32;
    let g = px.0[1] as i32;
    let b = px.0[2] as i32;
    let dominant_green = g - r.max(b);
    assert!(
        dominant_green < 32,
        "translucent polyline over magenta backdrop should blend to ~grey \
         (g - max(r,b) ≈ 0 under correct premul); got rgb=({r}, {g}, {b}), \
         green-dominance={dominant_green}. mesh.wgsl::fs probably forgot to \
         premultiply (see docs/review-wgsl-shaders.md A1)."
    );
}
