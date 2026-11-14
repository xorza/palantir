//! Shapes added straight to a node: rects, lines, polylines, curves,
//! arcs and triangles, and the order they paint in.

use palantir::glam::{UVec2, Vec2};
use palantir::widget::{CubicBezier, QuadraticBezier, Shape};
use palantir::{
    Block, Border, Chrome, ColorRamp, Configure, Corners, Gradient, Panel, Rect, RgbaF32, Sizing,
    Ui,
};

use crate::fixtures;
use crate::golden_name::GoldenName;
use crate::goldens;
use crate::visual_rig::VisualRig;
use palantir::LineCap;
use palantir::LineJoin;
use palantir::Paint;
use palantir::Stroke;

/// Pin the slot mechanism: three sub-rect shapes interleaved with two child Frames, each overlapping the children that paint under it, so pixels tell "right slot" from "all collapsed to slot 0".
///
/// Layout (220×60 hstack): red 0..30 (slot 0, under cyan child 0..60); green 30..90 (slot 1); yellow child 60..120; blue 90..150 (slot 2).
///
/// Expected: cyan(0..30), green(30..60), yellow(60..90), blue(90..150). Collapsed: cyan(0..60), yellow(60..120), blue(120..150).
#[test]
fn interleaved_shapes_paint_in_record_order() {
    GoldenName::InterleavedShapesPaintOrder.assert_scene(UVec2::new(220, 60), |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .padding(0.0)
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::rect().at(Rect::new(0.0, 0.0, 30.0, 60.0)),
                    RgbaF32::srgb(1.0, 0.0, 0.0),
                );
                Block::new()
                    .id_salt("cyan")
                    .chrome(Chrome::new(RgbaF32::srgb(0.0, 1.0, 1.0)))
                    .size((Sizing::fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(
                    Shape::rect().at(Rect::new(30.0, 0.0, 60.0, 60.0)),
                    RgbaF32::srgb(0.0, 1.0, 0.0),
                );
                Block::new()
                    .id_salt("yellow")
                    .chrome(Chrome::new(RgbaF32::srgb(1.0, 1.0, 0.0)))
                    .size((Sizing::fixed(60.0), Sizing::FILL))
                    .show(ui);
                ui.add_shape(
                    Shape::rect().at(Rect::new(90.0, 0.0, 60.0, 60.0)),
                    RgbaF32::srgb(0.2, 0.4, 1.0),
                );
            });
    });
}

/// Pin: `Shape::line` paints a fringe-AA stroke; a non-AA path stair-steps a diagonal and fails the channel tolerance.
#[test]
fn line_diagonal_aa_matches_golden() {
    GoldenName::LineDiagonalAa.assert_scene(UVec2::new(160, 120), |ui| {
        fixtures::canvas(ui, |ui| {
            ui.add_shape(
                Shape::line(Vec2::new(10.0, 10.0), Vec2::new(150.0, 110.0)),
                Stroke::new(RgbaF32::srgb(0.2, 0.9, 1.0), 4.0),
            );
            // Sub-pixel hairlines must appear dim, not vanish. On a pixel boundary (y = 80) a 0.4 px line splits 0.2 + 0.2 over two rows; through a center (y = 40.5) it lands 0.4 on one. Equal energy, so brightness does not pulse.
            for y in [80.0, 40.5] {
                ui.add_shape(
                    Shape::line(Vec2::new(10.0, y), Vec2::new(150.0, y)),
                    Stroke::new(RgbaF32::srgb(1.0, 1.0, 1.0), 0.4),
                );
            }
        });
    });
}

/// Pin: `Shape::polyline` with `per_point` colors paints a gradient by vertex interpolation; a stride-1 cross-section would collapse to single-color strips.
#[test]
fn polyline_gradient_matches_golden() {
    GoldenName::PolylineGradient.assert_scene(UVec2::new(160, 140), |ui| {
        fixtures::canvas(ui, |ui| {
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
                Shape::polyline(&pts).per_point(&cols),
                Stroke::new(RgbaF32::WHITE, 5.0),
            );
        });
    });
}

/// Pin: sharp Miter joins downgrade to a bevel instead of an unbounded spike. A join regression shows as missing pixels in the sharp stroke only.
#[test]
fn polyline_bevel_join_matches_golden() {
    GoldenName::PolylineBevelJoin.assert_scene(UVec2::new(180, 140), |ui| {
        fixtures::canvas(ui, |ui| {
            let cyan = RgbaF32::srgb(0.2, 0.9, 1.0);
            let shallow = [
                Vec2::new(15.0, 30.0),
                Vec2::new(60.0, 60.0),
                Vec2::new(105.0, 30.0),
            ];
            ui.add_shape(Shape::polyline(&shallow), Stroke::new(cyan, 5.0));
            let sharp = [
                Vec2::new(15.0, 100.0),
                Vec2::new(80.0, 115.0),
                Vec2::new(20.0, 130.0),
            ];
            ui.add_shape(Shape::polyline(&sharp), Stroke::new(cyan, 5.0));
        });
    });
}

/// Pin: `LineCap::Round` paints a half-disc fan at each endpoint, beside Butt and Square.
#[test]
fn polyline_round_caps_match_golden() {
    GoldenName::PolylineRoundCaps.assert_scene(UVec2::new(180, 140), |ui| {
        fixtures::canvas(ui, |ui| {
            for (y, cap, color) in [
                (30.0_f32, LineCap::Butt, RgbaF32::srgb(1.0, 0.4, 0.4)),
                (70.0, LineCap::Square, RgbaF32::srgb(0.4, 1.0, 0.4)),
                (110.0, LineCap::Round, RgbaF32::srgb(0.4, 0.6, 1.0)),
            ] {
                ui.add_shape(
                    Shape::line(Vec2::new(40.0, y), Vec2::new(140.0, y)),
                    Stroke::new(color, 10.0).with_cap(cap),
                );
            }
        });
    });
}

/// Pin: `LineJoin::Round` paints a circular arc at interior joins; Miter / Bevel / Round must differ visibly.
#[test]
fn polyline_round_join_matches_golden() {
    GoldenName::PolylineRoundJoin.assert_scene(UVec2::new(180, 200), |ui| {
        fixtures::canvas(ui, |ui| {
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
                ui.add_shape(
                    Shape::polyline(&pts),
                    Stroke::new(cyan, 8.0).with_join(join),
                );
            }
        });
    });
}

/// Pin: translucent polyline joints must not double-blend; an α=0.5 stroke stays α=0.5 through every joint.
#[test]
fn polyline_translucent_joins_have_uniform_coverage() {
    let mut h = VisualRig::new();
    let img = h
        .size(UVec2::new(180, 160))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            fixtures::canvas(ui, |ui| {
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
                        Shape::polyline(&pts),
                        Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 14.0).with_join(*join),
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
    GoldenName::PolylineTranslucentJoins.assert_matches(&img);
}

/// Pin: a translucent polyline blends correctly through `PREMULTIPLIED_ALPHA_BLENDING`. Green α=0.5 over magenta gives linear `(0.5, 0.5, 0.5)`, sRGB 188 gray; a straight-alpha source tints green (>220).
#[test]
fn polyline_translucent_premultiplies_in_stroke_shader() {
    let img = VisualRig::new()
        .size(UVec2::new(120, 120))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            fixtures::canvas(ui, |ui| {
                ui.add_shape(
                    Shape::rect().at(Rect::new(0.0, 0.0, 120.0, 120.0)),
                    RgbaF32::srgb(1.0, 0.0, 1.0),
                );
                let pts = [Vec2::new(10.0, 60.0), Vec2::new(110.0, 60.0)];
                ui.add_shape(
                    Shape::polyline(&pts),
                    Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 24.0),
                );
            });
        })
        .image;
    let gray = RgbaF32::new(0.5, 0.5, 0.5, 1.0).to_srgba8();
    fixtures::assert_px(
        img.get_pixel(60, 60).0,
        [gray.r, gray.g, gray.b, 255],
        "the stroke's center blends to gray; green means the stroke shader skipped premultiplying",
    );
}

/// Pin the curve pipeline: `Shape::cubic_bezier` lowers to `ShapeRecord::Stroke` and draws per scissor group. Three identical cubics with Butt / Square / Round caps differ only at the endpoints; a fourth quadratic pins quadratic→cubic promotion.
#[test]
fn curve_caps_match_golden() {
    GoldenName::CurveCaps.assert_scene(UVec2::new(220, 240), |ui| {
        fixtures::canvas(ui, |ui| {
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
                    Shape::cubic_bezier(CubicBezier {
                        p0: Vec2::new(30.0, dy + 40.0),
                        p1: Vec2::new(60.0, dy - 10.0),
                        p2: Vec2::new(140.0, dy - 10.0),
                        p3: Vec2::new(170.0, dy + 40.0),
                    }),
                    Stroke::new(*color, 8.0).with_cap(*cap),
                );
            }
            ui.add_shape(
                Shape::quadratic_bezier(QuadraticBezier {
                    p0: Vec2::new(30.0, 215.0),
                    p1: Vec2::new(100.0, 170.0),
                    p2: Vec2::new(170.0, 215.0),
                }),
                Stroke::new(RgbaF32::srgb(1.0, 0.85, 0.2), 4.0).with_cap(LineCap::Round),
            );
        });
    });
}

/// Pin the triangle forms: sharp vs rounded corners, solid vs fill+inner-stroke vs stroke-only, and both windings (bottom-right is CW).
#[test]
fn triangle_matches_golden() {
    GoldenName::Triangle.assert_scene(UVec2::new(240, 240), |ui| {
        fixtures::canvas(ui, |ui| {
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(20.0, 100.0),
                    Vec2::new(65.0, 15.0),
                    Vec2::new(110.0, 100.0),
                ),
                RgbaF32::srgb(1.0, 0.4, 0.4),
            );
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(130.0, 100.0),
                    Vec2::new(175.0, 15.0),
                    Vec2::new(220.0, 100.0),
                )
                .radius(12.0_f32),
                RgbaF32::srgb(0.4, 1.0, 0.5),
            );
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(20.0, 220.0),
                    Vec2::new(65.0, 135.0),
                    Vec2::new(110.0, 220.0),
                )
                .radius(8.0_f32),
                Paint::from(RgbaF32::srgb(0.2, 0.5, 1.0))
                    .with_border(Border::new(RgbaF32::WHITE, 3.0)),
            );
            ui.add_shape(
                Shape::triangle(
                    Vec2::new(220.0, 220.0),
                    Vec2::new(175.0, 135.0),
                    Vec2::new(130.0, 220.0),
                )
                .radius(6.0_f32),
                Paint::from(RgbaF32::TRANSPARENT)
                    .with_border(Border::new(RgbaF32::srgb(1.0, 0.85, 0.2), 3.0)),
            );
        });
    });
}

/// Pin: `Shape::arc` on the curve pipeline. A full ±2π circle closes seamlessly under Butt caps; a 3/4-sweep gradient arc fades and ends in a round head cap.
#[test]
fn arc_shapes_match_golden() {
    use std::f32::consts::{FRAC_PI_2, PI};
    GoldenName::ArcShapes.assert_scene(UVec2::new(180, 140), |ui| {
        fixtures::canvas(ui, |ui| {
            ui.add_shape(
                Shape::circle().at(Rect::new(15.0, 40.0, 60.0, 60.0)),
                Stroke::new(RgbaF32::srgb(0.2, 0.9, 1.0), 4.0),
            );
            let comet = ColorRamp::from([
                RgbaF32::srgb(1.0, 0.85, 0.2).with_alpha(0.0),
                RgbaF32::srgb(1.0, 0.85, 0.2),
            ]);
            ui.add_shape(
                Shape::arc(-FRAC_PI_2, 1.5 * PI).at(Rect::new(100.0, 40.0, 60.0, 60.0)),
                Stroke::new(RgbaF32::WHITE, 8.0)
                    .with_ramp(comet)
                    .with_cap(LineCap::Round),
            );
        });
    });
}

/// A rect off the pixel grid covers, per side, the pixel its edge crosses, even where that pixel's center is outside. With snap off, (40.75, 20.75)..(100.25, 60.25) covers column 40 and row 20 by 0.25 and column 100 and row 60 likewise; pixels just inside are full. Black over white leaves `1 − coverage`.
#[test]
fn an_unsnapped_rect_covers_the_pixels_across_each_edge() {
    let mut h = VisualRig::unsnapped();
    let img = h
        .size(UVec2::new(140, 80))
        .clear(RgbaF32::WHITE)
        .frame(|ui| {
            fixtures::canvas(ui, |ui| {
                ui.add_shape(
                    Shape::rect().at(Rect::new(40.75, 20.75, 59.5, 39.5)),
                    RgbaF32::BLACK,
                );
            });
        })
        .image;
    let encode = |coverage: f32| {
        RgbaF32::new(1.0 - coverage, 1.0 - coverage, 1.0 - coverage, 1.0)
            .to_srgba8()
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

/// The cases of [`a_template_matches_the_one_quad_form`], one to a cell: rounded and sharp rects
/// with and without borders, a border wider than its radius and one wider than half the box,
/// thin bars, mixed radii, capsules, circles, triangles, gradients and turned shapes,
/// all off the pixel grid and translucent, so a pixel shaded twice or missed moves its value.
fn template_sweep(ui: &mut Ui) {
    use palantir::widget::{PaintAnimation, PaintRepeat};
    use std::time::Duration;
    const CELL: f32 = 72.0;
    const COLS: usize = 8;
    let fill = RgbaF32::srgba(0.2, 0.55, 0.9, 0.7);
    let border =
        |w: f32| Paint::from(fill).with_border(Border::new(RgbaF32::srgba(1.0, 0.6, 0.1, 0.8), w));
    let turn = PaintAnimation::turn(0.07, 0.07)
        .with_started_at(Duration::ZERO)
        .with_period(Duration::from_secs(1))
        .with_repeat(PaintRepeat::Forever);
    let cell = |i: usize, w: f32, h: f32| {
        let x = (i % COLS) as f32 * CELL + 6.3;
        let y = (i / COLS) as f32 * CELL + 6.6;
        Rect::new(x, y, w, h)
    };
    fixtures::canvas(ui, |ui| {
        let mut i = 0;
        let mut next = || {
            i += 1;
            i - 1
        };
        for (corners, w) in [
            (Corners::all(0.0), 0.0),
            (Corners::all(9.5), 0.0),
            (Corners::all(9.5), 2.5),
            (Corners::all(3.0), 6.0),
            (Corners::new(0.0, 14.0, 4.0, 22.0), 1.5),
            (Corners::all(0.0), 30.0),
            (Corners::all(28.0), 0.0),
            (Corners::all(28.0), 3.0),
        ] {
            ui.add_shape(
                Shape::rect().at(cell(next(), 56.0, 56.0)).corners(corners),
                border(w),
            );
        }
        for (h, corners) in [(0.6, 0.0), (1.5, 0.0), (3.2, 1.0), (9.0, 4.5)] {
            ui.add_shape(
                Shape::rect().at(cell(next(), 58.0, h)).corners(corners),
                border(0.0),
            );
        }
        ui.add_shape(
            Shape::rect().at(cell(next(), 60.0, 24.0)).corners(12.0),
            border(2.0),
        );
        ui.add_shape(
            Shape::rect().at(cell(next(), 2.4, 50.0)).corners(1.2),
            border(0.0),
        );
        for d in [3.0, 9.0, 31.0, 57.0] {
            ui.add_shape(
                Shape::circle().at(cell(next(), d, d)),
                border(if d > 20.0 { 2.0 } else { 0.0 }),
            );
        }
        let tri = |i: usize| {
            let c = cell(i, 0.0, 0.0).min;
            (
                c + Vec2::new(4.0, 52.0),
                c + Vec2::new(30.0, 3.0),
                c + Vec2::new(57.0, 44.0),
            )
        };
        for (r, w) in [(0.0, 0.0), (6.0, 0.0), (6.0, 2.5), (1.0, 9.0)] {
            let (a, b, c) = tri(next());
            ui.add_shape(Shape::triangle(a, b, c).radius(r), border(w));
        }
        let (a, b, c) = tri(next());
        ui.add_shape(Shape::triangle(c, b, a).radius(3.0_f32), border(1.0));
        let at = cell(next(), 4.0, 4.0).min;
        ui.add_shape(
            Shape::triangle(at, at + Vec2::new(60.0, 2.0), at + Vec2::new(1.0, 8.0)),
            border(0.0),
        );
        ui.add_shape(
            Shape::rect().at(cell(next(), 56.0, 40.0)).corners(10.0),
            Gradient::linear(
                0.6,
                [
                    RgbaF32::srgba(1.0, 0.3, 0.2, 0.9),
                    RgbaF32::srgba(0.1, 0.8, 0.4, 0.5),
                ],
            ),
        );
        ui.add_shape(
            Shape::circle().at(cell(next(), 50.0, 50.0)),
            Gradient::centered_radial([
                RgbaF32::srgba(1.0, 1.0, 1.0, 0.9),
                RgbaF32::srgba(0.2, 0.3, 1.0, 0.4),
            ]),
        );
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(cell(next(), 44.0, 30.0)).corners(8.0),
            border(2.0),
        ) {
            shape.animate(turn);
        }
        let (a, b, c) = tri(next());
        if let Some(shape) = ui.add_shape(Shape::triangle(a, b, c).radius(4.0_f32), border(1.5)) {
            shape.animate(turn);
        }
    });
}

/// Every box, circle and triangle drawn as its template equals the one quad that measures its
/// distance at each pixel, to three 8-bit levels per channel: the template interpolates in each
/// region the closed form the one quad evaluates from its interpolated position, so the two
/// differ by the rounding of an f32 varying across a triangle, about 0.002 px of distance on a
/// 28 px arc, which the sRGB curve near black, 12.92 levels a linear level, turns into two levels
/// at a faint edge, and a turned shape's rotation of its vertices rounds once more: the turned
/// rounded rect reads three at one pixel near black. A pixel shaded by two triangles, or by none,
/// would move its translucent value by tens of levels, so the tolerance also proves the regions
/// tile the shape.
#[test]
fn a_template_matches_the_one_quad_form() {
    let size = UVec2::new(8 * 72, 5 * 72);
    let mut template = VisualRig::unsnapped();
    let mut one_quad = VisualRig::unsnapped();
    one_quad.host.disable_shape_template();
    let [template, one_quad] = [&mut template, &mut one_quad].map(|rig| {
        rig.size(size)
            .clear(RgbaF32::srgb(0.1, 0.1, 0.12))
            .frame(template_sweep)
            .image
    });
    let deltas: Vec<u8> = template
        .as_raw()
        .iter()
        .zip(one_quad.as_raw())
        .map(|(&a, &b)| a.abs_diff(b))
        .collect();
    let most = deltas.iter().copied().max().unwrap();
    if most > 3 {
        goldens::assert_same("template_against_one_quad", &template, &one_quad);
    }
    assert!(most <= 3, "a channel moved {most} levels");
}
