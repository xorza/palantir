//! Per-widget fixtures: smallest possible scene that exercises one
//! widget's render path.

use aperture::{
    Background, Brush, Button, Color, ColorU8, ComboBox, Configure, ConicGradient, Corners,
    DragValue, Frame, LineCap, LineJoin, LinearGradient, Modal, Panel, ProgressBar, RadialGradient,
    Rect, Shadow, Shape, Sizing, Slider, Spinner, Stroke, Switch, Text, ToggleTheme,
};
use glam::{UVec2, Vec2};
use image::Rgba;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

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
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("card")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Color::rgb(0.20, 0.30, 0.55).into(),
                        stroke: Stroke::solid(Color::rgb(0.65, 0.80, 1.00), 2.0),
                        corners: Corners::all(16.0),
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
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id_salt("card")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Brush::Linear(LinearGradient::two_stop(
                            FRAC_PI_2,
                            ColorU8::hex(0x1a1a2e),
                            ColorU8::hex(0x4c5cdb),
                        )),
                        corners: Corners::all(16.0),
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
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 180.0, 100.0)),
                    corners: Corners::all(12.0),
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

/// Pin the `Shape::WindowedRect` inverted-fill path end-to-end: bright
/// gradient content drawn as a plain unclipped rect, the windowed rect
/// over it filling the corner wedges with the scene background and
/// stroking the boundary — the cheap stand-in for rounded-corner
/// clipping. The golden must read as a rounded-clipped gradient card;
/// gradient corners bleeding past the stroke means the fill inversion
/// or the wedge coverage broke.
#[test]
fn windowed_rect_masks_corners_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                let card = Rect::new(0.0, 0.0, 180.0, 100.0);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(card),
                    corners: Corners::ZERO,
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        ColorU8::hex(0xff5e44),
                        ColorU8::hex(0xfacc15),
                    )),
                    stroke: Stroke::ZERO,
                });
                ui.add_shape(Shape::WindowedRect {
                    local_rect: Some(card),
                    corners: Corners::all(20.0),
                    fill: DARK_BG.into(),
                    stroke: Stroke::solid(Color::rgb(0.65, 0.80, 1.00), 2.0),
                });
            });
    });
    assert_matches_golden("windowed_rect_masks_corners", &img, Tolerance::default());
}

/// Render the showcase's gradients tab as a single golden so the
/// six demo cells are pinned end-to-end: two-stop horizontal /
/// vertical / 45°, three-stop, three spread modes stacked, three
/// interp spaces stacked. Acts as an eyeball-replacement for the
/// "open the showcase" check the slice-2 plan asked for, and locks
/// the visual against shader / atlas drift.
#[test]
fn showcase_gradients_tab_matches_golden() {
    use aperture::{Interp, Spread, Stop};
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
            corners: Corners::all(8.0),
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
                            .background(cell(LinearGradient::two_stop(FRAC_PI_2, navy, blue)))
                            .show(ui);
                        Frame::new()
                            .id_salt("diag")
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(cell(LinearGradient::two_stop(FRAC_PI_4, orange, yellow)))
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
                        corners: Corners::all(8.0),
                        ..Default::default()
                    })
                    .show(ui);
                let c = ConicGradient::new(
                    glam::Vec2::splat(0.5),
                    0.0,
                    [
                        aperture::Stop::new(0.0, ColorU8::hex(0xff5e44)),
                        aperture::Stop::new(0.25, ColorU8::hex(0xfacc15)),
                        aperture::Stop::new(0.5, ColorU8::hex(0x46c46c)),
                        aperture::Stop::new(0.75, ColorU8::hex(0x4c5cdb)),
                        aperture::Stop::new(1.0, ColorU8::hex(0xff5e44)),
                    ],
                );
                Frame::new()
                    .id_salt("conic")
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(Background {
                        fill: Brush::Conic(c),
                        corners: Corners::all(8.0),
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
                        corners: Corners::new(4.0, 12.0, 20.0, 28.0),
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
///
/// Panel rect `(-6, -6) .. (194, 144)` over a `120×90` viewport,
/// radius 24. Three of the four rounded corners (TR / BL / BR) are
/// fully off-screen; only TL pokes into the viewport — its arc center
/// at world `(18, 18)` makes the arc cross the viewport's top edge at
/// `x≈2.1` and left edge at `y≈2.1`, producing a small visible
/// green-stroked curve plus a `DARK_BG` corner cutout in the top-left.
/// The rest of the viewport is filled solid black.
///
/// Without the fix, the mask SDF uses the viewport-clamped scissor —
/// so additional spurious rounded notches appear at the TR / BL / BR
/// viewport corners where the bug-mode mask cuts the panel fill, and
/// `DARK_BG` shows through there too. The pixel asserts below pin
/// that exact discrimination.
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
                    .position(Vec2::new(-6.0, -6.0))
                    .size((Sizing::fixed(200.0), Sizing::fixed(150.0)))
                    .background(Background {
                        fill: Color::TRANSPARENT.into(),
                        stroke: Stroke::solid(Color::rgb_u8(0, 255, 0), 4.0),
                        corners: Corners::all(24.0),
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

    // sRGB(0.08, 0.08, 0.10) ≈ (20, 20, 25). "near-black" = all
    // channels well under that; "dark-bg-ish" = R/G near 20.
    let is_near_black = |p: Rgba<u8>| p.0[0] < 8 && p.0[1] < 8 && p.0[2] < 8;
    let is_dark_bg = |p: Rgba<u8>| p.0[0] > 12 && p.0[0] < 32 && p.0[2] > 12 && p.0[2] < 40;

    // TL viewport corner is the genuine cutout — DARK_BG should show
    // through whether the fix is in place or not.
    let tl = *img.get_pixel(0, 0);
    assert!(
        is_dark_bg(tl),
        "TL viewport corner should be DARK_BG (genuine offscreen-arc cutout), got rgba={:?}",
        tl.0,
    );

    // Discriminating pixels: the other three viewport corners must
    // be solid black under the fix. With the bug (viewport-clamped
    // mask), each gets a spurious rounded notch and reads DARK_BG.
    for (x, y, label) in [(119, 0, "TR"), (0, 89, "BL"), (119, 89, "BR")] {
        let px = *img.get_pixel(x, y);
        assert!(
            is_near_black(px),
            "{label} viewport corner ({x},{y}) should be black (panel arc is offscreen), \
             got rgba={:?} — the mask SDF is using the viewport-clamped scissor \
             instead of the panel's true rect.",
            px.0,
        );
    }

    // Viewport centre should obviously be black.
    let centre = *img.get_pixel(60, 45);
    assert!(
        is_near_black(centre),
        "viewport centre should be solid black, got rgba={:?}",
        centre.0,
    );

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
    let scene = |ui: &mut aperture::Ui| {
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
                        corners: Corners::all(8.0),
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
                    corners: Corners::default(),
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("cyan")
                    .background(Background {
                        fill: Color::rgb(0.0, 1.0, 1.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(30.0, 0.0, 60.0, 60.0)),
                    corners: Corners::default(),
                    fill: Color::rgb(0.0, 1.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                Frame::new()
                    .id_salt("yellow")
                    .background(Background {
                        fill: Color::rgb(1.0, 1.0, 0.0).into(),
                        ..Default::default()
                    })
                    .size((Sizing::fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(90.0, 0.0, 60.0, 60.0)),
                    corners: Corners::default(),
                    fill: Color::rgb(0.2, 0.4, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    });
    assert_matches_golden("interleaved_shapes_paint_order", &img, Tolerance::default());
}

/// Pin: `Shape::Line` paints a fringe-AA stroke. A diagonal 4-px
/// cyan line across a dark frame exercises the curve cmd →
/// composer → GPU stroke pipeline end-to-end. The AA fade is the
/// load-bearing visual signal — a non-AA stroke path would produce
/// a stair-stepped diagonal that fails the per-pixel channel
/// tolerance immediately.
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
                });
                // Hairlines at sub-pixel width — should appear dim
                // (coverage-faded) rather than vanish or look identical
                // to the 4 px stroke. Two alignments pin the trapezoid
                // coverage plateau: on a pixel *boundary* (y = 80) the
                // 0.4 px line splits 0.2 + 0.2 across two rows; through
                // a pixel *center* (y = 40.5) it lands 0.4 on one row —
                // equal total energy, so brightness doesn't pulse as a
                // hairline drifts across alignments.
                for y in [80.0, 40.5] {
                    ui.add_shape(Shape::Line {
                        a: Vec2::new(10.0, y),
                        b: Vec2::new(150.0, y),
                        width: 0.4,
                        brush: Color::rgb(1.0, 1.0, 1.0).into(),
                        cap: LineCap::Butt,
                    });
                }
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
    use aperture::PolylineColors;
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

/// Pin: sharp Miter polyline joins downgrade to a clean bevel
/// rather than an unbounded spike. Two strokes side by side: the
/// shallow 90° corner mitres, the tight chevron triggers the
/// bevel downgrade. Golden captures both at the same width so a
/// join-chrome regression (e.g. a flipped convex-side sign →
/// missing corner fill) shows up as missing pixels in the sharp
/// stroke only.
#[test]
fn polyline_bevel_join_matches_golden() {
    use aperture::PolylineColors;
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
                    });
                }
            });
    });
    assert_matches_golden("polyline_round_caps", &img, Tolerance::default());
}

/// Pin: `LineJoin::Round` paints a circular arc at interior joins.
/// Three identical 90° corners with Miter / Bevel / Round joins
/// — Miter shows a sharp point, Bevel a flat cut, Round a smooth
/// arc. Visually distinct golden ensures the join kind threads
/// through to the chrome instances and their fragment metrics.
#[test]
fn polyline_round_join_matches_golden() {
    use aperture::PolylineColors;
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

/// Pin: translucent polyline joints must not double-blend. The GPU
/// joint model clips adjacent segment strips at the angle bisector
/// (each fragment of the concave overlap belongs to exactly one
/// strip) and fills the convex wedge with one chrome instance — so an
/// α=0.5 stroke stays uniformly α=0.5 straight through every joint.
/// Probes the overlap wedge under each apex analytically on top of
/// the golden.
#[test]
fn polyline_translucent_joins_have_uniform_coverage() {
    use aperture::PolylineColors;
    let mut h = Harness::new();
    // Three translucent chevrons, one per join kind. The GPU joint
    // model clips adjacent segment strips at the angle bisector, so
    // their concave overlap is covered exactly once — a brighter
    // wedge under a corner means the partition regressed and the
    // strips double-blended.
    let img = h.render(UVec2::new(180, 160), 1.0, Color::BLACK, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (i, join) in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round]
                    .iter()
                    .enumerate()
                {
                    let y = 40.0 + i as f32 * 45.0;
                    let pts = [
                        Vec2::new(20.0, y),
                        Vec2::new(90.0, y - 25.0),
                        Vec2::new(160.0, y),
                    ];
                    ui.add_shape(Shape::Polyline {
                        points: &pts,
                        colors: PolylineColors::Single(Color::rgba(0.0, 1.0, 0.0, 0.5)),
                        width: 14.0,
                        cap: LineCap::Butt,
                        join: *join,
                    });
                }
            });
    });
    // Analytic probe, stronger than the golden: a point 4 px below
    // each apex sits inside the concave overlap wedge of the two
    // strips (behind A's end face, ahead of B's start face, well
    // within the stroke width). Its value must match a straight-run
    // interior pixel exactly — α 0.5 blended twice would jump the
    // green channel by ~35 sRGB steps.
    for i in 0..3u32 {
        let y = 40 + i * 45;
        let joint = img.get_pixel(90, y - 21);
        let straight = img.get_pixel(55, y - 12);
        assert!(
            joint[1].abs_diff(straight[1]) <= 2 && joint[0] == straight[0],
            "row {i}: joint interior {joint:?} != straight interior {straight:?} — \
             adjacent segments double-blended their concave overlap",
        );
    }
    assert_matches_golden("polyline_translucent_joins", &img, Tolerance::default());
}

/// Pin: a translucent polyline must blend through
/// `PREMULTIPLIED_ALPHA_BLENDING` correctly — the stroke pipeline's
/// fragment shader must premultiply its straight-alpha color at
/// output. The visual test paints a translucent green stroke
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
/// center pixel. A regression of the `curve.wgsl::fs` premultiply
/// step fails this with `delta ≈ 60+`.
#[test]
fn polyline_translucent_premultiplies_in_stroke_shader() {
    use aperture::PolylineColors;
    let mut h = Harness::new();
    // Backdrop + a 24px horizontal translucent green stroke at y=60.
    let img = h.render(UVec2::new(120, 120), 1.0, Color::BLACK, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 120.0, 120.0)),
                    corners: Corners::ZERO,
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
         premultiply."
    );
}

/// Pin the native GPU curve pipeline end-to-end: encoder lowers
/// `Shape::CubicBezier` to `ShapeRecord::Curve`, composer batches into
/// one `CurveBatch`, `CurvePipeline` issues a single
/// `pass.draw_indexed(0..96, ..)` per scissor group. Three cubic curves with
/// Butt / Square / Round caps, identical shape and width — the only
/// visual difference is the endpoint geometry, so the golden pins both
/// the strip and the cap-SDF code path. A fourth quadratic curve below
/// pins the quadratic→cubic promotion at lowering.
#[test]
fn curve_caps_match_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 240), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // Three identical "hill" cubics, one per cap kind.
                // Symmetric so the cap effect is the only delta.
                for (i, (cap, color)) in [
                    (LineCap::Butt, Color::rgb(1.0, 0.4, 0.4)),
                    (LineCap::Square, Color::rgb(0.4, 1.0, 0.4)),
                    (LineCap::Round, Color::rgb(0.4, 0.6, 1.0)),
                ]
                .iter()
                .enumerate()
                {
                    let dy = 20.0 + i as f32 * 55.0;
                    ui.add_shape(Shape::CubicBezier {
                        p0: Vec2::new(30.0, dy + 40.0),
                        p1: Vec2::new(60.0, dy - 10.0),
                        p2: Vec2::new(140.0, dy - 10.0),
                        p3: Vec2::new(170.0, dy + 40.0),
                        width: 8.0,
                        brush: (*color).into(),
                        cap: *cap,
                    });
                }
                // Quadratic curve at the bottom — exercises the
                // q→cubic promotion path.
                ui.add_shape(Shape::QuadraticBezier {
                    p0: Vec2::new(30.0, 215.0),
                    p1: Vec2::new(100.0, 170.0),
                    p2: Vec2::new(170.0, 215.0),
                    width: 4.0,
                    brush: Color::rgb(1.0, 0.85, 0.2).into(),
                    cap: LineCap::Round,
                });
            });
    });
    assert_matches_golden("curve_caps", &img, Tolerance::default());
}

/// Rounded-triangle SDF primitive: pins the `FillKind::TRIANGLE` shader
/// branch. Four triangles exercise the axes that matter — sharp vs rounded
/// corners, solid fill vs fill+inner-stroke vs stroke-only (transparent
/// fill), and both windings (the bottom-right is CW to prove the SDF's
/// winding-sign fold handles either orientation).
#[test]
fn triangle_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(240, 240), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // Top-left: sharp solid fill.
                ui.add_shape(Shape::Triangle {
                    a: Vec2::new(20.0, 100.0),
                    b: Vec2::new(65.0, 15.0),
                    c: Vec2::new(110.0, 100.0),
                    radius: 0.0,
                    fill: Color::rgb(1.0, 0.4, 0.4),
                    stroke: Stroke::ZERO,
                });
                // Top-right: rounded solid fill.
                ui.add_shape(Shape::Triangle {
                    a: Vec2::new(130.0, 100.0),
                    b: Vec2::new(175.0, 15.0),
                    c: Vec2::new(220.0, 100.0),
                    radius: 12.0,
                    fill: Color::rgb(0.4, 1.0, 0.5),
                    stroke: Stroke::ZERO,
                });
                // Bottom-left: rounded fill + inner-edge stroke.
                ui.add_shape(Shape::Triangle {
                    a: Vec2::new(20.0, 220.0),
                    b: Vec2::new(65.0, 135.0),
                    c: Vec2::new(110.0, 220.0),
                    radius: 8.0,
                    fill: Color::rgb(0.2, 0.5, 1.0),
                    stroke: Stroke::solid(Color::WHITE, 3.0),
                });
                // Bottom-right: stroke-only (transparent fill), CW winding.
                ui.add_shape(Shape::Triangle {
                    a: Vec2::new(220.0, 220.0),
                    b: Vec2::new(175.0, 135.0),
                    c: Vec2::new(130.0, 220.0),
                    radius: 6.0,
                    fill: Color::TRANSPARENT,
                    stroke: Stroke::solid(Color::rgb(1.0, 0.85, 0.2), 3.0),
                });
            });
    });
    assert_matches_golden("triangle", &img, Tolerance::default());
}

/// ProgressBar at 50%: the two-`Fill`-leaf split resolves to a
/// half-width accent fill over the rounded pill track.
#[test]
fn progress_bar_half_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(220, 60), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ProgressBar::new(0.5).id_salt("pb").show(ui);
            });
    });
    assert_matches_golden("progress_bar_half", &img, Tolerance::default());
}

/// Switch on + off with animation disabled: pins the knob at each
/// rest position and exercises the `Canvas` track + absolutely-positioned
/// knob path (the only widget that places a child via `.position`).
#[test]
fn toggle_switch_states_matches_golden() {
    let mut h = Harness::new();
    let mut style = ToggleTheme::switch(&aperture::Palette::DEFAULT);
    style.anim = None; // sit at the rest position, no first-frame transient
    let img = h.render(UVec2::new(220, 110), 1.0, DARK_BG, |ui| {
        let mut on = true;
        let mut off = false;
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Switch::new(&mut on)
                    .id_salt("on")
                    .label("on")
                    .style(&style)
                    .show(ui);
                Switch::new(&mut off)
                    .id_salt("off")
                    .label("off")
                    .style(&style)
                    .show(ui);
            });
    });
    assert_matches_golden("toggle_switch_states", &img, Tolerance::default());
}

/// Pin: `Shape::Arc` renders natively on the GPU curve pipeline. A
/// full ±2π circle closes seamlessly under Butt caps (no seam pixel
/// at angle 0); a 3/4-sweep gradient arc fades along its sweep and
/// terminates in a round head cap. Regressions in the arc basis
/// (angle mixing, tangent sign, cap SDF) show as gaps or flat ends.
#[test]
fn arc_shapes_match_golden() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let mut h = Harness::new();
    let img = h.render(UVec2::new(180, 140), 1.0, DARK_BG, |ui| {
        Panel::zstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::circle(Vec2::new(45.0, 70.0), 30.0, 4.0)
                        .brush(Color::rgb(0.2, 0.9, 1.0)),
                );
                let comet = LinearGradient::two_stop(
                    0.0,
                    Color::rgb(1.0, 0.85, 0.2).with_alpha(0.0),
                    Color::rgb(1.0, 0.85, 0.2),
                );
                ui.add_shape(
                    Shape::arc(Vec2::new(130.0, 70.0), 30.0, -FRAC_PI_2, 1.5 * PI, 8.0)
                        .brush(comet)
                        .cap(LineCap::Round),
                );
            });
    });
    assert_matches_golden("arc_shapes", &img, Tolerance::default());
}

/// Spinner at t=0 (phase 0): the comet arc renders as a round-capped
/// GPU arc whose gradient fades from transparent tail to full head.
#[test]
fn spinner_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(80, 80), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Spinner::new().size(48.0).id_salt("sp").show(ui);
            });
    });
    assert_matches_golden("spinner", &img, Tolerance::default());
}

/// Slider at 30%: the two-tone rail (accent left, grey right) splits at
/// the round knob via the `Fill`-weight trick — no record-time width.
#[test]
fn slider_thirty_percent_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(240, 60), 1.0, DARK_BG, |ui| {
        let mut v = 0.3_f32;
        Panel::vstack()
            .auto_id()
            .padding(20.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Slider::new(&mut v, 0.0..=1.0).id_salt("sl").show(ui);
            });
    });
    assert_matches_golden("slider_thirty_percent", &img, Tolerance::default());
}

/// DragValue renders its formatted number + suffix inside button chrome.
#[test]
fn drag_value_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(140, 64), 1.0, DARK_BG, |ui| {
        let mut v = 42.5_f64;
        Panel::vstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                DragValue::new(&mut v)
                    .decimals(1)
                    .suffix(" px")
                    .size((Sizing::fixed(100.0), Sizing::HUG))
                    .id_salt("dv")
                    .show(ui);
            });
    });
    assert_matches_golden("drag_value", &img, Tolerance::default());
}

/// ComboBox (closed): button-styled trigger showing the current choice
/// with a down-chevron drawn as a polyline (font-independent), right of
/// the label via `SpaceBetween`.
#[test]
fn combo_box_closed_matches_golden() {
    let mut h = Harness::new();
    let opts = ["Apple", "Banana", "Cherry"];
    let img = h.render(UVec2::new(220, 70), 1.0, DARK_BG, |ui| {
        let mut sel = 1usize;
        Panel::vstack()
            .auto_id()
            .padding(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ComboBox::new(&mut sel, &opts)
                    .size((Sizing::fixed(160.0), Sizing::HUG))
                    .id_salt("cb")
                    .show(ui);
            });
    });
    assert_matches_golden("combo_box_closed", &img, Tolerance::default());
}

/// Modal: a centered card over the dim backdrop, recorded into
/// `Layer::Modal` so it composites above the `Main` content behind it.
#[test]
fn modal_dialog_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(300, 200), 1.0, DARK_BG, |ui| {
        // Bright content behind, so the backdrop's dim is visible.
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .background(Background {
                fill: Color::rgb(0.35, 0.45, 0.65).into(),
                stroke: Stroke::ZERO,
                corners: Corners::ZERO,
                shadow: Shadow::NONE,
            })
            .show(ui, |_| {});
        Modal::new().id_salt("m").show(ui, |ui| {
            Text::new("Confirm?").id_salt("mt").show(ui);
        });
    });
    assert_matches_golden("modal_dialog", &img, Tolerance::default());
}
