//! Shapes added straight to a node: rects, lines, polylines, curves,
//! arcs and triangles, and the order they paint in.

use glam::{UVec2, Vec2};
use palantir::widget::{LineCap, LineJoin, Shape};
use palantir::{
    Background, Block, ColorRamp, Configure, LinearGradient, Panel, Rect, RgbaF32, Sizing, Stroke,
};

use crate::fixtures::DARK_BG;
use crate::golden_name::GoldenName;
use crate::goldens::assert_matches_golden;
use crate::harness::Harness;

/// Pin the `Shape::windowed_rect` inverted-fill path end-to-end: bright
/// gradient content drawn as a plain unclipped rect, the windowed rect
/// over it filling the corner wedges with the scene background and
/// stroking the boundary — the cheap stand-in for rounded-corner
/// clipping. The golden must read as a rounded-clipped gradient card;
/// gradient corners bleeding past the stroke means the fill inversion
/// or the wedge coverage broke.
#[test]
fn windowed_rect_masks_corners_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(220, 140))
        .frame(|ui| {
            Panel::vstack()
                .auto_id()
                .padding(20.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let card = Rect::new(0.0, 0.0, 180.0, 100.0);
                    ui.add_shape(Shape::rect(card).fill(LinearGradient::two_stop(
                        0.0,
                        RgbaF32::hex(0xff5e44),
                        RgbaF32::hex(0xfacc15),
                    )));
                    ui.add_shape(
                        Shape::windowed_rect(card)
                            .corners(20.0)
                            .fill(DARK_BG)
                            .border(Stroke::new(RgbaF32::srgb(0.65, 0.80, 1.00), 2.0)),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::WindowedRectMasksCorners, &img);
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
    let img = h
        .size(UVec2::new(220, 60))
        .frame(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(0.0)
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 30.0, 60.0))
                            .fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                    );
                    Block::new()
                        .id_salt("cyan")
                        .background(Background {
                            fill: RgbaF32::srgb(0.0, 1.0, 1.0).into(),
                            ..Default::default()
                        })
                        .size((Sizing::fixed(60.0), Sizing::FILL))
                        .show(ui);
                    ui.add_shape(
                        Shape::rect(Rect::new(30.0, 0.0, 60.0, 60.0))
                            .fill(RgbaF32::srgb(0.0, 1.0, 0.0)),
                    );
                    Block::new()
                        .id_salt("yellow")
                        .background(Background {
                            fill: RgbaF32::srgb(1.0, 1.0, 0.0).into(),
                            ..Default::default()
                        })
                        .size((Sizing::fixed(60.0), Sizing::FILL))
                        .show(ui);
                    ui.add_shape(
                        Shape::rect(Rect::new(90.0, 0.0, 60.0, 60.0))
                            .fill(RgbaF32::srgb(0.2, 0.4, 1.0)),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::InterleavedShapesPaintOrder, &img);
}

/// Pin: `Shape::line` paints a fringe-AA stroke. A diagonal 4-px
/// cyan line across a dark frame exercises the curve cmd →
/// composer → GPU stroke pipeline end-to-end. The AA fade is the
/// load-bearing visual signal — a non-AA stroke path would produce
/// a stair-stepped diagonal that fails the per-pixel channel
/// tolerance immediately.
#[test]
fn line_diagonal_aa_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(160, 120))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::line(
                        Vec2::new(10.0, 10.0),
                        Vec2::new(150.0, 110.0),
                        Stroke::new(RgbaF32::srgb(0.2, 0.9, 1.0), 4.0),
                    ));
                    // Hairlines at sub-pixel width — should appear dim
                    // (coverage-faded) rather than vanish or look identical
                    // to the 4 px stroke. Two alignments pin the trapezoid
                    // coverage plateau: on a pixel *boundary* (y = 80) the
                    // 0.4 px line splits 0.2 + 0.2 across two rows; through
                    // a pixel *center* (y = 40.5) it lands 0.4 on one row —
                    // equal total energy, so brightness doesn't pulse as a
                    // hairline drifts across alignments.
                    for y in [80.0, 40.5] {
                        ui.add_shape(Shape::line(
                            Vec2::new(10.0, y),
                            Vec2::new(150.0, y),
                            Stroke::new(RgbaF32::srgb(1.0, 1.0, 1.0), 0.4),
                        ));
                    }
                });
        })
        .image;
    assert_matches_golden(GoldenName::LineDiagonalAa, &img);
}

/// Pin: `Shape::polyline` with `per_point` colours paints
/// a multi-stop gradient via GPU vertex interpolation. A 4-point
/// zig-zag with four corner colors exercises the per-point
/// coloring + miter joins + composer arena copy in one frame. A
/// stride-1 inner cross-section would collapse to single-color
/// strips, which would fail the gradient sample tolerance.
#[test]
fn polyline_gradient_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(160, 140))
        .frame(|ui| {
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
                        RgbaF32::srgb(1.0, 0.2, 0.2),
                        RgbaF32::srgb(1.0, 0.85, 0.2),
                        RgbaF32::srgb(0.2, 1.0, 0.4),
                        RgbaF32::srgb(0.2, 0.6, 1.0),
                    ];
                    ui.add_shape(
                        Shape::polyline(&pts, Stroke::new(RgbaF32::WHITE, 5.0)).per_point(&cols),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::PolylineGradient, &img);
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
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(180, 140))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let cyan = RgbaF32::srgb(0.2, 0.9, 1.0);
                    let shallow = [
                        Vec2::new(15.0, 30.0),
                        Vec2::new(60.0, 60.0),
                        Vec2::new(105.0, 30.0),
                    ];
                    ui.add_shape(Shape::polyline(&shallow, Stroke::new(cyan, 5.0)));
                    let sharp = [
                        Vec2::new(15.0, 100.0),
                        Vec2::new(80.0, 115.0),
                        Vec2::new(20.0, 130.0),
                    ];
                    ui.add_shape(Shape::polyline(&sharp, Stroke::new(cyan, 5.0)));
                });
        })
        .image;
    assert_matches_golden(GoldenName::PolylineBevelJoin, &img);
}

/// Pin: `LineCap::Round` paints a half-disc fan at each endpoint
/// — visible as the rounded ends of a thick stroke. Golden also
/// compares Butt + Square + Round side by side: cap-style
/// regressions (e.g. Round collapsing to Butt) show up as missing
/// arc pixels at the end of the bottom stroke.
#[test]
fn polyline_round_caps_match_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(180, 140))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for (y, cap, color) in [
                        (30.0_f32, LineCap::Butt, RgbaF32::srgb(1.0, 0.4, 0.4)),
                        (70.0, LineCap::Square, RgbaF32::srgb(0.4, 1.0, 0.4)),
                        (110.0, LineCap::Round, RgbaF32::srgb(0.4, 0.6, 1.0)),
                    ] {
                        ui.add_shape(
                            Shape::line(
                                Vec2::new(40.0, y),
                                Vec2::new(140.0, y),
                                Stroke::new(color, 10.0),
                            )
                            .cap(cap),
                        );
                    }
                });
        })
        .image;
    assert_matches_golden(GoldenName::PolylineRoundCaps, &img);
}

/// Pin: `LineJoin::Round` paints a circular arc at interior joins.
/// Three identical 90° corners with Miter / Bevel / Round joins
/// — Miter shows a sharp point, Bevel a flat cut, Round a smooth
/// arc. Visually distinct golden ensures the join kind threads
/// through to the chrome instances and their fragment metrics.
#[test]
fn polyline_round_join_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(180, 200))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let cyan = RgbaF32::srgb(0.2, 0.9, 1.0);
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
                        ui.add_shape(Shape::polyline(&pts, Stroke::new(cyan, 8.0)).join(join));
                    }
                });
        })
        .image;
    assert_matches_golden(GoldenName::PolylineRoundJoin, &img);
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
    let mut h = Harness::new();
    // Three translucent chevrons, one per join kind. The GPU joint
    // model clips adjacent segment strips at the angle bisector, so
    // their concave overlap is covered exactly once — a brighter
    // wedge under a corner means the partition regressed and the
    // strips double-blended.
    let img = h
        .size(UVec2::new(180, 160))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
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
                        ui.add_shape(
                            Shape::polyline(
                                &pts,
                                Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 14.0),
                            )
                            .join(*join),
                        );
                    }
                });
        })
        .image;
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
        assert_eq!(
            joint, straight,
            "row {i}: the joint interior differs from a straight one — \
             adjacent segments double-blended their concave overlap",
        );
    }
    assert_matches_golden(GoldenName::PolylineTranslucentJoins, &img);
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
/// center pixel. A regression of the `curve_pipeline/shader.wgsl::fs` premultiply
/// step fails this with `delta ≈ 60+`.
#[test]
fn polyline_translucent_premultiplies_in_stroke_shader() {
    let mut h = Harness::new();
    // Backdrop + a 24px horizontal translucent green stroke at y=60.
    let img = h
        .size(UVec2::new(120, 120))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 120.0, 120.0))
                            .fill(RgbaF32::srgb(1.0, 0.0, 1.0)),
                    );
                    let pts = [Vec2::new(10.0, 60.0), Vec2::new(110.0, 60.0)];
                    ui.add_shape(Shape::polyline(
                        &pts,
                        Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 24.0),
                    ));
                });
        })
        .image;
    // Sample the stroke's center (x=60, y=60). RgbaImage is
    // sRGB-encoded after the swapchain target's auto-encode.
    let px = img.get_pixel(60, 60);
    let r = i32::from(px.0[0]);
    let g = i32::from(px.0[1]);
    let b = i32::from(px.0[2]);
    let dominant_green = g - r.max(b);
    assert!(
        dominant_green < 32,
        "translucent polyline over magenta backdrop should blend to ~grey \
         (g - max(r,b) ≈ 0 under correct premul); got rgb=({r}, {g}, {b}), \
         green-dominance={dominant_green}. mesh_pipeline/shader.wgsl::fs probably forgot to \
         premultiply."
    );
}

/// Pin the native GPU curve pipeline end-to-end: encoder lowers
/// `Shape::cubic_bezier` to `ShapeRecord::Curve`, composer batches into
/// one curve batch, `CurvePipeline` issues a single
/// `pass.draw_indexed(0..96, ..)` per scissor group. Three cubic curves with
/// Butt / Square / Round caps, identical shape and width — the only
/// visual difference is the endpoint geometry, so the golden pins both
/// the strip and the cap-SDF code path. A fourth quadratic curve below
/// pins the quadratic→cubic promotion at lowering.
#[test]
fn curve_caps_match_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(220, 240))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Three identical "hill" cubics, one per cap kind.
                    // Symmetric so the cap effect is the only delta.
                    for (i, (cap, color)) in [
                        (LineCap::Butt, RgbaF32::srgb(1.0, 0.4, 0.4)),
                        (LineCap::Square, RgbaF32::srgb(0.4, 1.0, 0.4)),
                        (LineCap::Round, RgbaF32::srgb(0.4, 0.6, 1.0)),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let dy = 20.0 + i as f32 * 55.0;
                        ui.add_shape(
                            Shape::cubic_bezier(
                                Vec2::new(30.0, dy + 40.0),
                                Vec2::new(60.0, dy - 10.0),
                                Vec2::new(140.0, dy - 10.0),
                                Vec2::new(170.0, dy + 40.0),
                                Stroke::new(*color, 8.0),
                            )
                            .cap(*cap),
                        );
                    }
                    // Quadratic curve at the bottom — exercises the
                    // q→cubic promotion path.
                    ui.add_shape(
                        Shape::quadratic_bezier(
                            Vec2::new(30.0, 215.0),
                            Vec2::new(100.0, 170.0),
                            Vec2::new(170.0, 215.0),
                            Stroke::new(RgbaF32::srgb(1.0, 0.85, 0.2), 4.0),
                        )
                        .cap(LineCap::Round),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::CurveCaps, &img);
}

/// Rounded-triangle SDF primitive: pins the `FillKind::TRIANGLE` shader
/// branch. Four triangles exercise the axes that matter — sharp vs rounded
/// corners, solid fill vs fill+inner-stroke vs stroke-only (transparent
/// fill), and both windings (the bottom-right is CW to prove the SDF's
/// winding-sign fold handles either orientation).
#[test]
fn triangle_matches_golden() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(240, 240))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Top-left: sharp solid fill.
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(20.0, 100.0),
                            Vec2::new(65.0, 15.0),
                            Vec2::new(110.0, 100.0),
                        )
                        .fill(RgbaF32::srgb(1.0, 0.4, 0.4)),
                    );
                    // Top-right: rounded solid fill.
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(130.0, 100.0),
                            Vec2::new(175.0, 15.0),
                            Vec2::new(220.0, 100.0),
                        )
                        .radius(12.0_f32)
                        .fill(RgbaF32::srgb(0.4, 1.0, 0.5)),
                    );
                    // Bottom-left: rounded fill + inner-edge stroke.
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(20.0, 220.0),
                            Vec2::new(65.0, 135.0),
                            Vec2::new(110.0, 220.0),
                        )
                        .radius(8.0_f32)
                        .fill(RgbaF32::srgb(0.2, 0.5, 1.0))
                        .border(Stroke::new(RgbaF32::WHITE, 3.0)),
                    );
                    // Bottom-right: stroke-only (transparent fill), CW winding.
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(220.0, 220.0),
                            Vec2::new(175.0, 135.0),
                            Vec2::new(130.0, 220.0),
                        )
                        .radius(6.0_f32)
                        .fill(RgbaF32::TRANSPARENT)
                        .border(Stroke::new(RgbaF32::srgb(1.0, 0.85, 0.2), 3.0)),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::Triangle, &img);
}

/// Pin: `Shape::arc` renders natively on the GPU curve pipeline. A
/// full ±2π circle closes seamlessly under Butt caps (no seam pixel
/// at angle 0); a 3/4-sweep gradient arc fades along its sweep and
/// terminates in a round head cap. Regressions in the arc basis
/// (angle mixing, tangent sign, cap SDF) show as gaps or flat ends.
#[test]
fn arc_shapes_match_golden() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(180, 140))
        .frame(|ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::circle(
                        Vec2::new(45.0, 70.0),
                        30.0,
                        Stroke::new(RgbaF32::srgb(0.2, 0.9, 1.0), 4.0),
                    ));
                    let comet = ColorRamp::two_stop(
                        RgbaF32::srgb(1.0, 0.85, 0.2).with_alpha(0.0),
                        RgbaF32::srgb(1.0, 0.85, 0.2),
                    );
                    ui.add_shape(
                        Shape::arc(
                            Vec2::new(130.0, 70.0),
                            30.0,
                            -FRAC_PI_2,
                            1.5 * PI,
                            Stroke::new(RgbaF32::WHITE, 8.0),
                        )
                        .ramp(comet)
                        .cap(LineCap::Round),
                    );
                });
        })
        .image;
    assert_matches_golden(GoldenName::ArcShapes, &img);
}

/// A triangle with no area has no inside, so a radius grows its edges and
/// corners alone: three collinear corners paint a 4 px stadium along their
/// segment, three coincident ones a 6 px disc. Neither fills the quad the
/// radius inflates.
///
/// The stadium runs from (10, 20) to (90, 20), so its quad is x 6..94,
/// y 16..24. Pixel (6, 16) has its centre at (6.5, 16.5), 4.95 px from the
/// end at (10, 20): outside by almost a pixel. The disc sits at (50, 50)
/// in the quad 44..56; pixel (44, 44)'s centre is 7.8 px from it.
#[test]
fn a_degenerate_rounded_triangle_paints_its_edges_not_its_quad() {
    let mut h = Harness::new();
    let img = h
        .size(UVec2::new(100, 70))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::canvas()
                .id_salt("degenerate")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(10.0, 20.0),
                            Vec2::new(50.0, 20.0),
                            Vec2::new(90.0, 20.0),
                        )
                        .fill(RgbaF32::WHITE)
                        .radius(4.0_f32),
                    );
                    ui.add_shape(
                        Shape::triangle(Vec2::splat(50.0), Vec2::splat(50.0), Vec2::splat(50.0))
                            .fill(RgbaF32::WHITE)
                            .radius(6.0_f32),
                    );
                });
        })
        .image;
    let lit = |x: u32, y: u32| img.get_pixel(x, y).0[0] > 200;
    let dark = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 40;

    assert!(lit(50, 20), "the stadium's spine");
    assert!(lit(50, 17), "3.5 px from the spine, inside the 4 px radius");
    assert!(dark(6, 16), "the stadium's quad corner");
    assert!(dark(93, 23), "the stadium's other quad corner");
    assert!(lit(50, 50), "the disc's centre");
    assert!(dark(44, 44), "the disc's quad corner");
    assert!(dark(55, 55), "the disc's other quad corner");
}
