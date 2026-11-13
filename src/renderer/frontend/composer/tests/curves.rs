//! Polylines, arcs and cubics: the instances they emit and the join instances at their
//! joints.

use crate::common::span::Span;
use crate::internals::compose_rig::ComposeRig;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::composer::tests::curve_builder::CurveBuilder;
use crate::renderer::frontend::composer::tests::support::{
    TestPolyline, clip, image, lanes, params, rows_of, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_stroke_payload::{
    DrawStrokePayload, PolylinePath, StrokePath,
};
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::shape::geometry::stroke_geometry::PolylineColorSpan;
use crate::shape::polyline::PolylineColors;
use crate::shape::record_store::RecordStore;
use glam::{UVec2, Vec2};
use std::f32::consts::FRAC_PI_2;

/// A higher-kind stroke (a polyline, riding the curve tier) recorded between two
/// text runs splits the batch where it covers the first run, and only there:
/// strokes paint over text by kind order and an open batch emits at the end of a
/// later group, so merged text would paint over its stroke.
#[test]
fn compose_polyline_over_prior_text_splits_text_batch() {
    #[derive(Debug)]
    struct Case {
        stroke_y: f32,
        text_runs: usize,
    }
    // The first run spans y 0..20; the stroke reaches `stroke_y` ± 1 with its
    // fringe, so y = 10 lands inside the run and y = 25 clears it.
    for case in [
        Case {
            stroke_y: 10.0,
            text_runs: 2,
        },
        Case {
            stroke_y: 25.0,
            text_runs: 1,
        },
    ] {
        let buf = run(
            |b, store| {
                text(b, Rect::new(0.0, 0.0, 100.0, 20.0));
                TestPolyline::new(
                    &[
                        Vec2::new(0.0, case.stroke_y),
                        Vec2::new(100.0, case.stroke_y),
                    ],
                    1.0,
                )
                .record(b, store);
                text(b, Rect::new(0.0, 40.0, 100.0, 20.0));
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        assert_eq!(rows_of(&buf, Lane::Text).len(), case.text_runs, "{case:?}");
        assert_eq!(
            rows_of(&buf, Lane::Tier(PaintTier::Curve)),
            [Span::new(0, 1)],
            "one segment instance for a 2-point polyline: {case:?}",
        );
        assert!(buf.meshes.is_empty(), "no CPU-tessellated mesh: {case:?}");
    }
}

/// Polyline lowering: N points emit N−1 segment instances (user caps only on the
/// true ends, neighbor points on the joint lanes) plus N−2 join instances,
/// all in the curve stream.
#[test]
fn compose_polyline_emits_segments_and_join_chrome() {
    use crate::renderer::render_buffer::curve_caps::{CapEnds, CurveCaps};
    use crate::renderer::render_buffer::curve_kind::CurveKind;
    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(110.0, 10.0),
        Vec2::new(160.0, 40.0),
    ];
    let mut commands = PaintCapture::default();
    let mut store = RecordStore::default();
    TestPolyline::new(&pts, 4.0)
        .cap(LineCap::Round)
        .join(LineJoin::Round)
        .record(&mut commands, &mut store);
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    rig.store = store;
    rig.compose(&commands);
    let segs: Vec<_> = rig
        .out
        .curves
        .iter()
        .filter(|c| c.kind == CurveKind::SEGMENT)
        .collect();
    let joins: Vec<_> = rig
        .out
        .curves
        .iter()
        .filter(|c| c.kind == CurveKind::JOIN_ROUND)
        .collect();
    assert_eq!(segs.len(), 3);
    assert_eq!(joins.len(), 2);
    assert_eq!(rig.out.curves.len(), 5, "nothing else in the stream");

    let d0 = (pts[1] - pts[0]).normalize();
    let d1 = (pts[2] - pts[1]).normalize();
    let d2 = (pts[3] - pts[2]).normalize();
    assert_eq!(rig.composer.polyline.directions(), [d0, d1, d2]);
    // First segment: cap at its start only; the start plane lane is zero and the
    // end lane carries the pre-oriented bisector normal.
    assert_eq!(segs[0].p0, pts[0]);
    assert_eq!(segs[0].p3, pts[1]);
    assert_eq!(segs[0].p1, Vec2::ZERO, "no clip plane at a cap end");
    assert_eq!(segs[0].p2, d0 + d1, "end bisector plane rides p2");
    assert_eq!(segs[0].caps, CurveCaps::new(LineCap::Round, CapEnds::Start));
    // Interior segment: no caps, planes on both lanes; the start plane is the
    // bit-exact negation of the previous end plane (the overlap-partition
    // contract).
    assert_eq!(
        segs[1].caps,
        CurveCaps::new(LineCap::Round, CapEnds::Neither)
    );
    assert_eq!(
        segs[1].p1, -segs[0].p2,
        "shared joint planes negate exactly"
    );
    assert_eq!(segs[1].p2, d1 + d2);
    assert_eq!(segs[2].caps, CurveCaps::new(LineCap::Round, CapEnds::End));
    assert_eq!(
        segs[2].p1, -segs[1].p2,
        "shared joint planes negate exactly"
    );
    assert_eq!(segs[2].p2, Vec2::ZERO, "no clip plane at a cap end");
    assert_eq!(joins[0].p0, pts[1]);
    assert_eq!(joins[0].p1, -d0);
    assert_eq!(joins[0].p2, d1);
    assert_eq!(joins[1].p0, pts[2]);
    assert_eq!(joins[1].p1, -d1);
    assert_eq!(joins[1].p2, d2);
}

/// A joint between two differently colored segments paints their premultiplied
/// average: opaque red beside transparent black is red at half alpha.
#[test]
fn a_join_between_colors_averages_them_premultiplied() {
    use crate::primitives::paint::color::rgba_f16::RgbaF16;
    use crate::renderer::render_buffer::curve_kind::CurveKind;

    let red = RgbaF32::new(1.0, 0.0, 0.0, 1.0);
    let mut commands = PaintCapture::default();
    let mut store = RecordStore::default();
    TestPolyline::new(
        &[
            Vec2::new(10.0, 10.0),
            Vec2::new(60.0, 40.0),
            Vec2::new(110.0, 10.0),
            Vec2::new(160.0, 40.0),
        ],
        4.0,
    )
    .colors(PolylineColors::PerSegment(&[
        red,
        RgbaF32::TRANSPARENT,
        red,
    ]))
    .cap(LineCap::Round)
    .join(LineJoin::Round)
    .record(&mut commands, &mut store);
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    rig.store = store;
    rig.compose(&commands);
    let joins: Vec<RgbaF16> = rig
        .out
        .curves
        .iter()
        .filter(|c| c.kind == CurveKind::JOIN_ROUND)
        .map(|c| c.color0)
        .collect();
    let half_red = RgbaF32::new(1.0, 0.0, 0.0, 0.5).to_f16();
    assert_eq!(joins, [half_red, half_red]);
}

/// Miter joins downgrade to bevel past MITER_LIMIT and keep miter on
/// gentle bends (the SVG convention).
#[test]
fn compose_polyline_miter_downgrades_to_bevel_when_sharp() {
    use crate::renderer::render_buffer::curve_kind::CurveKind;
    let emit = |pts: [Vec2; 3]| {
        run(
            |b, store| {
                TestPolyline::new(&pts, 4.0).record(b, store);
            },
            &params(1.0, UVec2::new(300, 300)),
        )
    };
    let gentle = emit([
        Vec2::new(10.0, 10.0),
        Vec2::new(100.0, 10.0),
        Vec2::new(100.0, 100.0),
    ]);
    assert_eq!(
        gentle
            .curves
            .iter()
            .filter(|c| c.kind == CurveKind::JOIN_MITER)
            .count(),
        1,
    );
    let sharp = emit([
        Vec2::new(10.0, 10.0),
        Vec2::new(100.0, 10.0),
        Vec2::new(10.0, 27.0),
    ]);
    assert_eq!(
        sharp
            .curves
            .iter()
            .filter(|c| c.kind == CurveKind::JOIN_BEVEL)
            .count(),
        1,
        "sharp miter must downgrade to bevel chrome",
    );
}

/// PerPoint colors land on the segment's color/color1 lanes (the GPU lerps along
/// t); PerSegment paints each segment solid with the join at its neighbors'
/// midpoint. Coincident points are skipped and their colors dropped, as in the CPU
/// walker.
#[test]
fn compose_polyline_color_modes_and_coincident_skip() {
    use crate::renderer::render_buffer::curve_kind::CurveKind;
    let red = RgbaF32::srgb(1.0, 0.0, 0.0);
    let green = RgbaF32::srgb(0.0, 1.0, 0.0);
    let blue = RgbaF32::srgb(0.0, 0.0, 1.0);
    let red16 = red.to_f16();
    let green16 = green.to_f16();
    let blue16 = blue.to_f16();

    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(110.0, 10.0),
    ];
    let buf = run(
        |b, store| {
            TestPolyline::new(&pts, 4.0)
                .colors(PolylineColors::PerPoint(&[red, green, green, blue]))
                .join(LineJoin::Round)
                .record(b, store);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let segs: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CurveKind::SEGMENT)
        .collect();
    assert_eq!(segs.len(), 2, "duplicate point contributes no segment");
    assert_eq!((segs[0].color0, segs[0].color1), (red16, green16));
    assert_eq!((segs[1].color0, segs[1].color1), (green16, blue16));
    let join = buf
        .curves
        .iter()
        .find(|c| c.kind == CurveKind::JOIN_ROUND)
        .unwrap();
    assert_eq!(join.color0, green16, "PerPoint chrome = the joint color");

    let buf = run(
        |b, store| {
            TestPolyline::new(&pts, 4.0)
                .colors(PolylineColors::PerSegment(&[red, green, blue]))
                .join(LineJoin::Round)
                .record(b, store);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let segs: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CurveKind::SEGMENT)
        .collect();
    assert_eq!(segs.len(), 2);
    assert_eq!((segs[0].color0, segs[0].color1), (red16, red16));
    assert_eq!((segs[1].color0, segs[1].color1), (blue16, blue16));
    let join = buf
        .curves
        .iter()
        .find(|c| c.kind == CurveKind::JOIN_ROUND)
        .unwrap();
    assert_eq!(
        join.color0,
        RgbaF16::new(0.5, 0.0, 0.5, 1.0),
        "PerSegment chrome = linear midpoint of adjacent segment colors",
    );

    // The stroke's color multiplies each recorded color where it is drawn, and stands alone for a
    // polyline of no colors: `tint = (0.5, 0.25, 1, 0.5)` takes red to `(0.5, 0, 0, 0.5)`, green to
    // `(0, 0.25, 0, 0.5)` and blue to `(0, 0, 1, 0.5)`, each exact in f16.
    let tint = RgbaF32::new(0.5, 0.25, 1.0, 0.5);
    let (per_point, per_segment) = ([red, green, green, blue], [red, green, blue]);
    let segments = |colors| {
        let buf = run(
            |b, store| {
                TestPolyline::new(&pts, 4.0)
                    .colors(colors)
                    .color(tint)
                    .record(b, store);
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        buf.curves
            .iter()
            .filter(|c| c.kind == CurveKind::SEGMENT)
            .map(|c| (c.color0, c.color1))
            .collect::<Vec<_>>()
    };
    let [red_t, green_t, blue_t] = [
        RgbaF16::new(0.5, 0.0, 0.0, 0.5),
        RgbaF16::new(0.0, 0.25, 0.0, 0.5),
        RgbaF16::new(0.0, 0.0, 1.0, 0.5),
    ];
    assert_eq!(
        segments(PolylineColors::PerPoint(&per_point)),
        [(red_t, green_t), (green_t, blue_t)]
    );
    assert_eq!(
        segments(PolylineColors::PerSegment(&per_segment)),
        [(red_t, red_t), (blue_t, blue_t)]
    );
    assert_eq!(
        segments(PolylineColors::Single),
        [(tint.to_f16(), tint.to_f16()); 2]
    );
}

#[test]
fn compose_emits_one_curve_batch_per_scissor_group() {
    let buf = run(
        |b, _arena| {
            for offset in [0.0_f32, 50.0] {
                CurveBuilder::cubic(
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    [
                        Vec2::new(offset, 0.0),
                        Vec2::new(offset + 10.0, 50.0),
                        Vec2::new(offset + 90.0, 50.0),
                        Vec2::new(offset + 100.0, 0.0),
                    ],
                )
                .draw(b);
            }
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    // Each curve's control polygon is ≈ 181.98 px; `TARGET_CHORD_PX = 1.5` splits
    // it into 122 chords and `SEGMENTS_PER_INSTANCE = 16` packs those into 8
    // sub-instances. The two curves are one shape translated, so the run carries
    // twice that, every emitted instance.
    assert_eq!(
        rows_of(&buf, Lane::Tier(PaintTier::Curve)),
        [Span::new(0, 16)],
        "one run per group"
    );
    assert_eq!(buf.curves.len(), 16);
}

#[test]
fn compose_splits_curve_batches_across_scissor_groups() {
    let buf = run(
        |b, _arena| {
            CurveBuilder::cubic(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 50.0),
                    Vec2::new(90.0, 50.0),
                    Vec2::new(100.0, 0.0),
                ],
            )
            .draw(b);
            clip(b, Rect::new(0.0, 0.0, 50.0, 200.0));
            CurveBuilder::cubic(
                Rect::new(0.0, 0.0, 50.0, 50.0),
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(5.0, 25.0),
                    Vec2::new(45.0, 25.0),
                    Vec2::new(50.0, 0.0),
                ],
            )
            .draw(b);
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let scissors: Vec<Option<URect>> = buf.runs.iter().map(|run| run.scissor).collect();
    assert_eq!(
        (lanes(&buf), scissors),
        (
            vec![Lane::Tier(PaintTier::Curve); 2],
            vec![None, Some(URect::new(0, 0, 50, 200))]
        ),
        "a scissor change closes the group, and the curves of each list as a run of their own",
    );
}

/// A ramp curve's fill reaches every sub-instance whole: ramp tag, atlas row, and
/// the stroke color in both color lanes. A solid curve carries the solid tag and
/// the fallback row.
#[test]
fn compose_threads_curve_fill_kind_and_lut_row_into_instances() {
    use crate::primitives::packed::fill_kind::FillKind;
    let tint = RgbaF32::new(0.25, 0.5, 1.0, 0.75).to_f16();
    let buf = run(
        |b, _arena| {
            CurveBuilder::cubic(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 50.0),
                    Vec2::new(90.0, 50.0),
                    Vec2::new(100.0, 0.0),
                ],
            )
            .fill(GpuFill::curve(tint, Some(LutRow(7))))
            .width(4.0)
            .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(
        !buf.curves.is_empty(),
        "must emit at least one sub-instance"
    );
    for ci in &buf.curves {
        assert_eq!(ci.fill_kind, FillKind::RAMP);
        assert_eq!(
            ci.fill_lut_row,
            LutRow(7),
            "row threaded through to instance"
        );
        assert_eq!(
            (ci.color0, ci.color1),
            (tint, tint),
            "the tint rides both lanes"
        );
    }

    let solid = GpuFill::curve(tint, None);
    assert_eq!(
        (solid.color, solid.kind, solid.lut_row),
        (tint, FillKind::SOLID, LutRow::FALLBACK),
    );
}

#[test]
fn compose_arc_scales_geometry_and_subdivides_by_exact_length() {
    use crate::renderer::render_buffer::curve_kind::CurveKind;
    use std::f32::consts::PI;
    let sweep = 1.5 * PI;
    let buf = run(
        |b, _arena| {
            CurveBuilder::arc(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                CircularArc {
                    center: Vec2::new(50.0, 50.0),
                    radius: 20.0,
                    start_angle: 0.0,
                    sweep,
                },
            )
            .draw(b);
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    // Arc length = r_phys · sweep ≈ 188.5 px; segments = ⌈188.5 / 1.5⌉ = 126;
    // instances = ⌈126 / 16⌉ = 8.
    assert_eq!(buf.curves.len(), 8, "exact-length subdivision");
    for (i, ci) in buf.curves.iter().enumerate() {
        assert_eq!(ci.kind, CurveKind::ARC);
        assert_eq!(ci.p0, Vec2::new(100.0, 100.0), "center at DPI 2");
        assert_eq!(ci.p1.x, 40.0, "radius at DPI 2");
        assert_eq!(ci.p2, Vec2::new(0.0, sweep), "angles pass through");
        assert_eq!(ci.width, 4.0, "stroke width at DPI 2");
        let n = buf.curves.len() as f32;
        assert_eq!(ci.t0, i as f32 / n);
        if i + 1 == buf.curves.len() {
            assert_eq!(ci.t1, 1.0);
        }
    }
    assert_eq!(
        rows_of(&buf, Lane::Tier(PaintTier::Curve)),
        [Span::new(0, 8)]
    );
}

#[test]
fn compose_arc_spin_rotates_center_about_bbox_pivot_and_offsets_angles() {
    use std::f32::consts::{FRAC_PI_2, PI};
    // Pivot = bbox.center() = (50, 50); center (70, 50) spun by π/2 (clockwise,
    // y-down) maps (+20, 0) to (0, +20), so it lands at (50, 70).
    let buf = run(
        |b, _arena| {
            CurveBuilder::arc(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                CircularArc {
                    center: Vec2::new(70.0, 50.0),
                    radius: 10.0,
                    start_angle: 0.0,
                    sweep: PI,
                },
            )
            .turned(FRAC_PI_2)
            .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(!buf.curves.is_empty());
    for ci in &buf.curves {
        assert_eq!(
            ci.p0,
            Vec2::new(50.0, 70.0),
            "center rotated about the bbox pivot, got {:?}",
            ci.p0
        );
        assert_eq!(ci.p2.x, FRAC_PI_2, "a0 offset by rotation");
        assert_eq!(ci.p2.y, PI + FRAC_PI_2, "a1 offset");
    }
}

#[test]
fn compose_flat_cubic_emits_single_instance_curved_emits_many() {
    // Same 800 px span: a straight cubic collapses to one instance; a curved one
    // subdivides (⌈⌈800/1.5⌉/16⌉ = 34).
    let straight = |b: &mut PaintCapture| {
        CurveBuilder::cubic(
            Rect::new(0.0, 0.0, 800.0, 10.0),
            [
                Vec2::new(0.0, 5.0),
                Vec2::new(800.0 / 3.0, 5.0),
                Vec2::new(1600.0 / 3.0, 5.0),
                Vec2::new(800.0, 5.0),
            ],
        )
        .draw(b);
    };
    let curved = |b: &mut PaintCapture| {
        CurveBuilder::cubic(
            Rect::new(0.0, 0.0, 800.0, 400.0),
            [
                Vec2::new(0.0, 5.0),
                Vec2::new(266.0, 400.0),
                Vec2::new(533.0, 400.0),
                Vec2::new(800.0, 5.0),
            ],
        )
        .draw(b);
    };
    let vp = params(1.0, UVec2::new(900, 900));
    let flat_buf = run(|b, _| straight(b), &vp);
    let curved_buf = run(|b, _| curved(b), &vp);
    assert_eq!(flat_buf.curves.len(), 1, "flat fast-path: one instance");
    assert_eq!(flat_buf.curves[0].t0, 0.0);
    assert_eq!(flat_buf.curves[0].t1, 1.0);
    assert!(
        curved_buf.curves.len() > 10,
        "curved cubic keeps adaptive density, got {}",
        curved_buf.curves.len(),
    );
}

/// Pivot (50, 50) of the logical frame: a π/2 spin maps an offset (dx, dy) to (-dy, dx), so p0
/// (70, 50) → (50, 70) and p3 (50, 30) → (70, 50). The same points held from an origin (20, 0),
/// as the encoder hands a curve from its owner's position, turn about that same pivot, (30, 50)
/// from the origin, to the same pixels.
#[test]
fn compose_curve_spin_rotates_control_points_about_bbox_pivot() {
    use std::f32::consts::FRAC_PI_2;
    let points = [
        Vec2::new(70.0, 50.0),
        Vec2::new(70.0, 40.0),
        Vec2::new(60.0, 30.0),
        Vec2::new(50.0, 30.0),
    ];
    for origin in [Vec2::ZERO, Vec2::new(20.0, 0.0)] {
        let buf = run(
            |b, _arena| {
                CurveBuilder::cubic(
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    points.map(|q| q - origin),
                )
                .origin(origin)
                .turned(FRAC_PI_2)
                .draw(b);
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        assert!(!buf.curves.is_empty());
        let ci = &buf.curves[0];
        assert_eq!(ci.p0, Vec2::new(50.0, 70.0), "{origin}: {:?}", ci.p0);
        assert_eq!(ci.p1, Vec2::new(60.0, 70.0), "{origin}: {:?}", ci.p1);
        assert_eq!(ci.p2, Vec2::new(70.0, 60.0), "{origin}: {:?}", ci.p2);
        assert_eq!(ci.p3, Vec2::new(70.0, 50.0), "{origin}: {:?}", ci.p3);
    }
}

#[test]
fn compose_arc_and_curve_share_one_batch_per_group() {
    use crate::renderer::render_buffer::curve_kind::CurveKind;
    let buf = run(
        |b, _arena| {
            CurveBuilder::arc(
                Rect::new(0.0, 0.0, 40.0, 40.0),
                CircularArc {
                    center: Vec2::new(20.0, 20.0),
                    radius: 10.0,
                    start_angle: 0.0,
                    sweep: 1.0,
                },
            )
            .draw(b);
            CurveBuilder::cubic(
                Rect::new(100.0, 0.0, 100.0, 100.0),
                [
                    Vec2::new(100.0, 0.0),
                    Vec2::new(110.0, 50.0),
                    Vec2::new(190.0, 50.0),
                    Vec2::new(200.0, 0.0),
                ],
            )
            .draw(b);
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(
        rows_of(&buf, Lane::Tier(PaintTier::Curve)),
        [Span::new(0, buf.curves.len() as u32)],
        "arcs batch with cubics"
    );
    assert!(buf.curves.iter().any(|c| c.kind == CurveKind::ARC));
    assert!(buf.curves.iter().any(|c| c.kind == CurveKind::CUBIC));
}

#[test]
fn two_point_polyline_does_not_reserve_miter_join_reach() {
    #[derive(Debug)]
    struct Case {
        points: &'static [Vec2],
        lanes: [Lane; 2],
    }

    static TWO: [Vec2; 2] = [Vec2::new(0.0, 10.0), Vec2::new(20.0, 10.0)];
    static THREE: [Vec2; 3] = [
        Vec2::new(0.0, 10.0),
        Vec2::new(10.0, 0.0),
        Vec2::new(20.0, 10.0),
    ];
    let cases = [
        Case {
            points: &TWO,
            lanes: [Lane::Tier(PaintTier::Image), Lane::Tier(PaintTier::Curve)],
        },
        Case {
            points: &THREE,
            lanes: [Lane::Tier(PaintTier::Curve), Lane::Tier(PaintTier::Image)],
        },
    ];

    for case in cases {
        let buf = run(
            |b, store| {
                TestPolyline::new(case.points, 2.0).record(b, store);
                image(b, Rect::new(22.0, 0.0, 10.0, 10.0));
            },
            &params(1.0, UVec2::new(100, 100)),
        );
        assert_eq!(lanes(&buf), case.lanes, "{case:?}");
    }
}

/// The composer turns each polyline point about its pivot in the logical frame before the
/// ancestor transform: a centered horizontal segment from (15, 50) to (85, 50), turned a quarter
/// about (50, 50), runs from (50, 15) to (50, 85), and so does the same segment held from an
/// origin (10, 0), its points (5, 50) and (75, 50), as the encoder hands a polyline from its
/// owner's position.
#[test]
fn compose_spins_polyline_about_bbox_center() {
    let points = [Vec2::new(15.0, 50.0), Vec2::new(85.0, 50.0)];
    let origin = Vec2::new(10.0, 0.0);
    let held = points.map(|q| q - origin);
    let aabb = |spin: Option<Spin>, points: &[Vec2], origin: Vec2| -> (Vec2, Vec2) {
        let buf = run(
            |b, store| {
                let polyline = TestPolyline::new(points, 2.0).origin(origin);
                match spin {
                    Some(spin) => polyline.turned(spin),
                    None => polyline,
                }
                .record(b, store);
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        assert_eq!(buf.curves.len(), 1, "one segment instance");
        let ci = &buf.curves[0];
        (ci.p0.min(ci.p3), ci.p0.max(ci.p3))
    };
    let turned = Spin {
        pivot: Vec2::splat(50.0),
        angle: FRAC_PI_2,
    };
    assert_eq!(aabb(None, &points, Vec2::ZERO), (points[0], points[1]));
    let upright = (Vec2::new(50.0, 15.0), Vec2::new(50.0, 85.0));
    assert_eq!(aabb(Some(turned), &points, Vec2::ZERO), upright);
    assert_eq!(aabb(None, &held, origin), (points[0], points[1]));
    assert_eq!(aabb(Some(turned), &held, origin), upright);
}

/// `PaintPayload::check` only debug-asserts a polyline's geometry, which is safe because a
/// degenerate polyline slipping through in release emits nothing. This test holds that. It calls
/// the sink directly, below the assert, as `PaintCapture::replay` does.
#[test]
fn degenerate_polyline_emits_nothing_rather_than_panicking() {
    for points_len in [0u32, 1] {
        let buf = run(
            |b, arena| {
                arena.polyline_points.push(Vec2::ZERO);
                b.stroke(DrawStrokePayload {
                    path: StrokePath::Polyline(PolylinePath {
                        bbox: Rect::new(0.0, 0.0, 4.0, 4.0),
                        fit: FitMap::IDENTITY,
                        points: Span::new(0, points_len),
                        colors: PolylineColorSpan::Single,
                        join: LineJoin::Miter,
                    }),
                    origin: Vec2::ZERO,
                    fill: GpuFill {
                        color: RgbaF32::WHITE.to_f16(),
                        ..GpuFill::default()
                    },
                    width: 2.0,
                    ..Default::default()
                });
            },
            &params(1.0, UVec2::new(64, 64)),
        );
        assert!(
            buf.curves.is_empty(),
            "points_len={points_len} emitted geometry"
        );
    }
}

/// A turned stroke and a turned mesh are culled by their bounds at this frame's angle, not by the
/// square they sweep. A thin level line from (60, 100) to (140, 100), turned a quarter about
/// (100, 100), stands upright at x = 100 from y = 60 to 140. A clip at (60, 60)..(80, 80) lies
/// inside the square it sweeps, (60, 60)..(140, 140), and misses the line, so neither the
/// polyline, the cubic nor the mesh of it draws; a clip at (95, 60)..(105, 80) holds a piece of
/// it, so each draws.
#[test]
fn a_turned_draw_is_culled_by_its_bound_at_its_angle() {
    use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
    use crate::renderer::render_buffer::curve_kind::CurveKind;

    let turn = Spin {
        pivot: Vec2::splat(100.0),
        angle: FRAC_PI_2,
    };
    let points = [Vec2::new(60.0, 100.0), Vec2::new(140.0, 100.0)];
    for (what, scissor, draws) in [
        ("beside", Rect::new(60.0, 60.0, 20.0, 20.0), false),
        ("across", Rect::new(95.0, 60.0, 10.0, 20.0), true),
    ] {
        let buf = run(
            |b, store| {
                clip(b, scissor);
                TestPolyline::new(&points, 2.0)
                    .turned(turn)
                    .record(b, store);
                CurveBuilder::cubic(
                    Rect::new(60.0, 60.0, 80.0, 80.0),
                    [
                        Vec2::new(60.0, 100.0),
                        Vec2::new(80.0, 100.0),
                        Vec2::new(120.0, 100.0),
                        Vec2::new(140.0, 100.0),
                    ],
                )
                .turned(FRAC_PI_2)
                .draw(b);
                b.draw(
                    DrawMeshPayload {
                        bbox: Rect::new(60.0, 99.0, 80.0, 2.0),
                        origin: Vec2::ZERO,
                        fit: FitMap::IDENTITY,
                        tint: RgbaF32::WHITE.to_f16(),
                        vertices: Span::new(0, 3),
                        idxs: Span::new(0, 3),
                        spin: Some(turn),
                    },
                    1.0,
                );
                b.pop_clip();
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        let drew = |kind: CurveKind| buf.curves.iter().any(|c| c.kind == kind);
        assert_eq!(drew(CurveKind::SEGMENT), draws, "polyline {what}");
        assert_eq!(drew(CurveKind::CUBIC), draws, "cubic {what}");
        assert_eq!(!buf.meshes.is_empty(), draws, "mesh {what}");
    }
}
