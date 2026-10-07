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

/// Pin `Shape::windowed_rect`'s inverted fill: the windowed rect over gradient content fills the corner wedges with the background and strokes the boundary. The golden must read as a rounded-clipped card.
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

/// Pin the slot mechanism: three sub-rect shapes interleaved with two child Frames, each overlapping the children that paint under it, so pixels tell "right slot" from "all collapsed to slot 0".
///
/// Layout (220×60 hstack): red 0..30 (slot 0, under cyan child 0..60); green 30..90 (slot 1); yellow child 60..120; blue 90..150 (slot 2).
///
/// Expected: cyan(0..30), green(30..60), yellow(60..90), blue(90..150). Collapsed: cyan(0..60), yellow(60..120), blue(120..150).
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

/// Pin: `Shape::line` paints a fringe-AA stroke; a non-AA path stair-steps a diagonal and fails the channel tolerance.
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
                    // Sub-pixel hairlines must appear dim, not vanish. On a pixel boundary (y = 80) a 0.4 px line splits 0.2 + 0.2 over two rows; through a centre (y = 40.5) it lands 0.4 on one. Equal energy, so brightness does not pulse.
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

/// Pin: `Shape::polyline` with `per_point` colours paints a gradient by vertex interpolation; a stride-1 cross-section would collapse to single-colour strips.
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

/// Pin: sharp Miter joins downgrade to a bevel instead of an unbounded spike. A join regression shows as missing pixels in the sharp stroke only.
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

/// Pin: `LineCap::Round` paints a half-disc fan at each endpoint, beside Butt and Square.
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

/// Pin: `LineJoin::Round` paints a circular arc at interior joins; Miter / Bevel / Round must differ visibly.
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

/// Pin: translucent polyline joints must not double-blend; an α=0.5 stroke stays α=0.5 through every joint.
#[test]
fn polyline_translucent_joins_have_uniform_coverage() {
    let mut h = Harness::new();
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
    // Analytic probe: a point 4 px below each apex lies in the concave overlap and must equal a straight-run pixel. Double-blending α 0.5 would raise green by ~35 sRGB steps.
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

/// Pin: a translucent polyline blends correctly through `PREMULTIPLIED_ALPHA_BLENDING`. Green α=0.5 over magenta should give linear `(0.5, 0.5, 0.5)`, ~188 grey in sRGB; a straight-alpha source tints green (>220). Asserts `green - max(red, blue) < 32` at the centre.
#[test]
fn polyline_translucent_premultiplies_in_stroke_shader() {
    let mut h = Harness::new();
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

/// Pin the curve pipeline: `Shape::cubic_bezier` lowers to `ShapeRecord::Curve` and draws per scissor group. Three identical cubics with Butt / Square / Round caps differ only at the endpoints; a fourth quadratic pins quadratic→cubic promotion.
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

/// Pin the `FillKind::TRIANGLE` shader branch: sharp vs rounded corners, solid vs fill+inner-stroke vs stroke-only, and both windings (bottom-right is CW).
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
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(20.0, 100.0),
                            Vec2::new(65.0, 15.0),
                            Vec2::new(110.0, 100.0),
                        )
                        .fill(RgbaF32::srgb(1.0, 0.4, 0.4)),
                    );
                    ui.add_shape(
                        Shape::triangle(
                            Vec2::new(130.0, 100.0),
                            Vec2::new(175.0, 15.0),
                            Vec2::new(220.0, 100.0),
                        )
                        .radius(12.0_f32)
                        .fill(RgbaF32::srgb(0.4, 1.0, 0.5)),
                    );
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

/// Pin: `Shape::arc` on the curve pipeline. A full ±2π circle closes seamlessly under Butt caps; a 3/4-sweep gradient arc fades and ends in a round head cap.
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

/// A triangle with no area has no inside, so a radius grows only its edges and corners: three collinear corners paint a 4 px stadium, three coincident ones a 6 px disc, neither filling the inflated quad.
///
/// The stadium runs (10, 20) to (90, 20), quad x 6..94, y 16..24; pixel (6, 16)'s centre (6.5, 16.5) is 4.95 px from (10, 20), outside. The disc at (50, 50) has quad 44..56; pixel (44, 44)'s centre is 7.8 px away.
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

/// A rect off the pixel grid covers, per side, the pixel its edge crosses, even where that pixel's centre is outside. With snap off, (40.75, 20.75)..(100.25, 60.25) covers column 40 and row 20 by 0.25 and column 100 and row 60 likewise; pixels just inside are full. Black over white leaves `1 − coverage`.
#[test]
fn an_unsnapped_rect_covers_the_pixels_across_each_edge() {
    let mut h = Harness::new_with_pixel_snap(false);
    let img = h
        .size(UVec2::new(140, 80))
        .clear(RgbaF32::WHITE)
        .frame(|ui| {
            Panel::canvas()
                .id_salt("unsnapped")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(40.75, 20.75, 59.5, 39.5)).fill(RgbaF32::BLACK),
                    );
                });
        })
        .image;
    let encode = |coverage: f32| {
        RgbaF32::new(1.0 - coverage, 1.0 - coverage, 1.0 - coverage, 1.0)
            .to_srgba_u8()
            .r
    };
    for (x, y, coverage) in [
        (39, 40, 0.0),
        (40, 40, 0.25),
        (41, 40, 1.0),
        (99, 40, 1.0),
        (100, 40, 0.25),
        (101, 40, 0.0),
        (70, 19, 0.0),
        (70, 20, 0.25),
        (70, 21, 1.0),
        (70, 59, 1.0),
        (70, 60, 0.25),
        (70, 61, 0.0),
    ] {
        let got = img.get_pixel(x, y).0[0];
        let want = encode(coverage);
        assert!(
            got.abs_diff(want) <= 1,
            "({x}, {y}) is covered {coverage}: got {got}, want {want}",
        );
    }
}
