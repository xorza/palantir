//! Polylines, arcs and cubics: the instances they emit and the chrome at
//! their joins.

use crate::primitives::lut_row::LutRow;
use crate::primitives::{color::Color, color::ColorU8};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::tests::support::{
    clip, composer, curve, image, mesh, params, polyline_cmd, rect, render_buffer, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::stroke_bounds::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::record_payloads::RecordPayloads;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Pin: a higher-kind stroke (a polyline, riding the curve tier)
/// recorded between two text runs splits the batch. Strokes paint
/// over text by kind order; if it weren't a split, the merged batch's
/// text would emit at end-of-batch, *after* the stroke, breaking that
/// ordering.
#[test]
fn compose_polyline_between_texts_splits_text_batch() {
    let buf = run(
        |b, payloads| {
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            polyline_cmd(
                b,
                payloads,
                &[Vec2::new(0.0, 25.0), Vec2::new(100.0, 25.0)],
                &[Color::WHITE],
                ColorMode::Single,
                1.0,
                LineCap::Butt,
                LineJoin::Miter,
            );
            text(b, rect(0.0, 40.0, 100.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.text_batches.len(),
        2,
        "polyline between texts must split the batch",
    );
    // A polyline lowers to GPU stroke instances riding the curve
    // batches — a 2-point polyline is one segment, no join chrome.
    assert_eq!(buf.batches(PaintTier::Curve).len(), 1);
    assert_eq!(
        buf.batches(PaintTier::Curve)[0].items.len,
        1,
        "one segment instance for a 2-point polyline",
    );
    assert!(buf.meshes.is_empty(), "no CPU-tessellated mesh");
}

/// Slice-2 polyline lowering: an N-point polyline emits N−1 segment
/// instances (user caps only on the true ends, neighbor points on the
/// joint lanes) plus N−2 join-chrome instances of the user's join
/// kind, all in the curve stream.
#[test]
fn compose_polyline_emits_segments_and_join_chrome() {
    use crate::renderer::render_buffer::curve::{
        CURVE_KIND_JOIN_ROUND, CURVE_KIND_SEGMENT, cap_lanes,
    };
    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(110.0, 10.0),
        Vec2::new(160.0, 40.0),
    ];
    let mut commands = PaintCapture::default();
    let mut payloads = RecordPayloads::default();
    polyline_cmd(
        &mut commands,
        &mut payloads,
        &pts,
        &[Color::WHITE],
        ColorMode::Single,
        4.0,
        LineCap::Round,
        LineJoin::Round,
    );
    let mut composer = composer();
    let mut buf = render_buffer();
    composer
        .begin(
            params(1.0, UVec2::new(200, 200)),
            Duration::ZERO,
            &payloads,
            &mut buf,
        )
        .replay_from(&commands);
    let segs: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CURVE_KIND_SEGMENT)
        .collect();
    let joins: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CURVE_KIND_JOIN_ROUND)
        .collect();
    assert_eq!(segs.len(), 3);
    assert_eq!(joins.len(), 2);
    assert_eq!(buf.curves.len(), 5, "nothing else in the stream");

    let round = LineCap::Round as u32;
    let d0 = (pts[1] - pts[0]).normalize();
    let d1 = (pts[2] - pts[1]).normalize();
    let d2 = (pts[3] - pts[2]).normalize();
    assert_eq!(composer.polyline.directions, [d0, d1, d2]);
    // First segment: user cap at start, butt at joint end; the start
    // plane lane is zero (cap end, no clip) and the end lane carries
    // the pre-oriented bisector normal.
    assert_eq!(segs[0].p0, pts[0]);
    assert_eq!(segs[0].p3, pts[1]);
    assert_eq!(segs[0].p1, Vec2::ZERO, "no clip plane at a cap end");
    assert_eq!(segs[0].p2, d0 + d1, "end bisector plane rides p2");
    assert_eq!(segs[0].cap, cap_lanes(round, 0));
    // Interior segment: butt both ends, planes on both lanes. The
    // start plane must be the bit-exact negation of the previous
    // segment's end plane — the overlap-partition contract.
    assert_eq!(segs[1].cap, cap_lanes(0, 0));
    assert_eq!(
        segs[1].p1, -segs[0].p2,
        "shared joint planes negate exactly"
    );
    assert_eq!(segs[1].p2, d1 + d2);
    // Last segment: butt at joint, user cap at the true end.
    assert_eq!(segs[2].cap, cap_lanes(0, round));
    assert_eq!(
        segs[2].p1, -segs[1].p2,
        "shared joint planes negate exactly"
    );
    assert_eq!(segs[2].p2, Vec2::ZERO, "no clip plane at a cap end");
    // Chrome anchors at the interior points with the pre-oriented
    // face-plane normals (`p1 = -d_a`, `p2 = d_b`).
    assert_eq!(joins[0].p0, pts[1]);
    assert_eq!(joins[0].p1, -d0);
    assert_eq!(joins[0].p2, d1);
    assert_eq!(joins[1].p0, pts[2]);
    assert_eq!(joins[1].p1, -d1);
    assert_eq!(joins[1].p2, d2);
}

/// Miter joins downgrade to bevel chrome past MITER_LIMIT (sharp
/// bends), keep miter chrome on gentle ones — the SVG convention.
#[test]
fn compose_polyline_miter_downgrades_to_bevel_when_sharp() {
    use crate::renderer::render_buffer::curve::{CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER};
    let emit = |pts: [Vec2; 3]| {
        run(
            |b, payloads| {
                polyline_cmd(
                    b,
                    payloads,
                    &pts,
                    &[Color::WHITE],
                    ColorMode::Single,
                    4.0,
                    LineCap::Butt,
                    LineJoin::Miter,
                );
            },
            &params(1.0, UVec2::new(300, 300)),
        )
    };
    // Gentle 90° bend: cos(half angle) = cos 45° ≈ 0.707 > 1/4.
    let gentle = emit([
        Vec2::new(10.0, 10.0),
        Vec2::new(100.0, 10.0),
        Vec2::new(100.0, 100.0),
    ]);
    assert_eq!(
        gentle
            .curves
            .iter()
            .filter(|c| c.kind == CURVE_KIND_JOIN_MITER)
            .count(),
        1,
    );
    // Near-fold: turn ≈ 169°, cos(half angle) ≈ 0.095 < 1/4 → bevel.
    let sharp = emit([
        Vec2::new(10.0, 10.0),
        Vec2::new(100.0, 10.0),
        Vec2::new(10.0, 27.0),
    ]);
    assert_eq!(
        sharp
            .curves
            .iter()
            .filter(|c| c.kind == CURVE_KIND_JOIN_BEVEL)
            .count(),
        1,
        "sharp miter must downgrade to bevel chrome",
    );
}

/// PerPoint colors land on the segment's color/color1 lanes (GPU
/// lerps along t); PerSegment paints each segment solid with its own
/// color and the chrome with the midpoint of its neighbors. Coincident
/// points are skipped and their colors dropped, mirroring the CPU
/// walker's kept-point discipline.
#[test]
fn compose_polyline_color_modes_and_coincident_skip() {
    use crate::renderer::render_buffer::curve::{CURVE_KIND_JOIN_ROUND, CURVE_KIND_SEGMENT};
    let red = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let red8: ColorU8 = red.into();
    let green8: ColorU8 = green.into();
    let blue8: ColorU8 = blue.into();

    // PerPoint with a duplicated middle point: the duplicate is
    // dropped, and the kept segments read the colors at the original
    // point indices (0, 1) and (1, 3).
    let pts = [
        Vec2::new(10.0, 10.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(60.0, 40.0),
        Vec2::new(110.0, 10.0),
    ];
    let buf = run(
        |b, payloads| {
            polyline_cmd(
                b,
                payloads,
                &pts,
                &[red, green, green, blue],
                ColorMode::PerPoint,
                4.0,
                LineCap::Butt,
                LineJoin::Round,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let segs: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CURVE_KIND_SEGMENT)
        .collect();
    assert_eq!(segs.len(), 2, "duplicate point contributes no segment");
    assert_eq!((segs[0].color0, segs[0].color1), (red8, green8));
    assert_eq!((segs[1].color0, segs[1].color1), (green8, blue8));
    let join = buf
        .curves
        .iter()
        .find(|c| c.kind == CURVE_KIND_JOIN_ROUND)
        .unwrap();
    assert_eq!(join.color0, green8, "PerPoint chrome = the joint color");

    // PerSegment: solid lanes per segment; the skipped middle point
    // drops the degenerate segment's color (index 1), so the kept
    // segments paint colors 0 and 2 and the chrome their midpoint.
    let buf = run(
        |b, payloads| {
            polyline_cmd(
                b,
                payloads,
                &pts,
                &[red, green, blue],
                ColorMode::PerSegment,
                4.0,
                LineCap::Butt,
                LineJoin::Round,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let segs: Vec<_> = buf
        .curves
        .iter()
        .filter(|c| c.kind == CURVE_KIND_SEGMENT)
        .collect();
    assert_eq!(segs.len(), 2);
    assert_eq!((segs[0].color0, segs[0].color1), (red8, red8));
    assert_eq!((segs[1].color0, segs[1].color1), (blue8, blue8));
    let join = buf
        .curves
        .iter()
        .find(|c| c.kind == CURVE_KIND_JOIN_ROUND)
        .unwrap();
    assert_eq!(
        join.color0,
        red8.midpoint(blue8),
        "PerSegment chrome = midpoint of adjacent segment colors",
    );
}

#[test]
fn compose_emits_one_curve_batch_per_scissor_group() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    let buf = run(
        |b, _arena| {
            // Two curves under one (implicit) scissor group → must
            // batch into a single curve-tier batch. That's the load-bearing
            // promise: one draw call per scissor group, no matter how
            // many curves the group contains.
            for offset in [0.0_f32, 50.0] {
                b.draw_curve(DrawCurvePayload {
                    bounds: StrokeBounds::Still(rect(0.0, 0.0, 100.0, 100.0)),
                    origin: Vec2::ZERO,
                    basis: CurveBasis::Cubic {
                        p0: Vec2::new(offset, 0.0),
                        p1: Vec2::new(offset + 10.0, 50.0),
                        p2: Vec2::new(offset + 90.0, 50.0),
                        p3: Vec2::new(offset + 100.0, 0.0),
                    },
                    color: Color::WHITE.into(),
                    width: 2.0,
                    ..Default::default()
                });
            }
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.batches(PaintTier::Curve).len(),
        1,
        "one batch per group"
    );
    let batch = buf.batches(PaintTier::Curve)[0];
    assert_eq!(batch.last_group, 0);
    // Sub-instance count depends on adaptive subdivision, but both
    // curves contribute the *same* per-curve count (identical shape),
    // so the total must be ≥ 2 and even.
    assert!(batch.items.len >= 2 && batch.items.len.is_multiple_of(2));
    assert_eq!(
        buf.curves.len() as u32,
        batch.items.len,
        "batch covers every emitted instance",
    );
}

#[test]
fn compose_splits_curve_batches_across_scissor_groups() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(0.0, 0.0, 100.0, 100.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Cubic {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(10.0, 50.0),
                    p2: Vec2::new(90.0, 50.0),
                    p3: Vec2::new(100.0, 0.0),
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
            clip(b, rect(0.0, 0.0, 50.0, 200.0));
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(0.0, 0.0, 50.0, 50.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Cubic {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(5.0, 25.0),
                    p2: Vec2::new(45.0, 25.0),
                    p3: Vec2::new(50.0, 0.0),
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.batches(PaintTier::Curve).len(),
        2,
        "scissor change closes the open batch and opens a new one",
    );
    assert!(
        buf.batches(PaintTier::Curve)[0].last_group < buf.batches(PaintTier::Curve)[1].last_group,
        "batches anchor to monotonically increasing groups",
    );
}

#[test]
fn compose_threads_curve_fill_kind_and_lut_row_into_instances() {
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::fill_kind::FillKind;
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    let buf = run(
        |b, _arena| {
            // Linear gradient curve: fill_kind low byte = 1, lut_row = 7.
            // Every sub-instance must carry the same fill_kind and row.
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(0.0, 0.0, 100.0, 100.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Cubic {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(10.0, 50.0),
                    p2: Vec2::new(90.0, 50.0),
                    p3: Vec2::new(100.0, 0.0),
                },
                color: Color::TRANSPARENT.into(),
                width: 4.0,
                fill_kind: FillKind::linear(Spread::Pad),
                fill_lut_row: LutRow(7),
                ..Default::default()
            });
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(
        !buf.curves.is_empty(),
        "must emit at least one sub-instance"
    );
    for ci in &buf.curves {
        assert_eq!(ci.fill_kind.0 & 0xFF, 1, "linear brush low byte");
        assert_eq!(
            ci.fill_lut_row,
            LutRow(7),
            "row threaded through to instance"
        );
    }
}

#[test]
fn compose_arc_scales_geometry_and_subdivides_by_exact_length() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::renderer::render_buffer::curve::CURVE_KIND_ARC;
    use crate::scene::shapes::paint::CurveBasis;
    use std::f32::consts::PI;
    // 3/4 arc: r = 20 logical, sweep = 1.5π, at DPI scale 2.
    let sweep = 1.5 * PI;
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(0.0, 0.0, 100.0, 100.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Arc {
                    center: Vec2::new(50.0, 50.0),
                    radius: 20.0,
                    a0: 0.0,
                    a1: sweep,
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    // Arc length = r_phys · sweep = 40 · 1.5π ≈ 188.5 px. Segments =
    // ⌈188.5 / 1.5⌉ = 126; instances = ⌈126 / 16⌉ = 8.
    assert_eq!(buf.curves.len(), 8, "exact-length subdivision");
    for (i, ci) in buf.curves.iter().enumerate() {
        assert_eq!(ci.kind, CURVE_KIND_ARC);
        // Center → physical px (DPI 2), radius scaled, angles verbatim.
        assert_eq!(ci.p0, Vec2::new(100.0, 100.0), "center at DPI 2");
        assert_eq!(ci.p1.x, 40.0, "radius at DPI 2");
        assert_eq!(ci.p2, Vec2::new(0.0, sweep), "angles pass through");
        assert_eq!(ci.width, 4.0, "stroke width at DPI 2");
        // t ranges tile [0, 1] contiguously, ending exactly at 1.
        let n = buf.curves.len() as f32;
        assert!((ci.t0 - i as f32 / n).abs() < 1e-6);
        if i + 1 == buf.curves.len() {
            assert_eq!(ci.t1, 1.0);
        }
    }
    // One batch covers every instance — arcs ride the curve batching.
    assert_eq!(buf.batches(PaintTier::Curve).len(), 1);
    assert_eq!(buf.batches(PaintTier::Curve)[0].items.len, 8);
}

#[test]
fn compose_arc_spin_rotates_center_about_bbox_pivot_and_offsets_angles() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    use std::f32::consts::{FRAC_PI_2, PI};
    // Pivot = bbox.center() = (50, 50); center (70, 50) is +20 along x.
    // rotation = π/2 (clockwise on screen, y-down): (+20, 0) → (0, +20),
    // so the spun center is (50, 70). Both angles shift by π/2.
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Spun {
                    spin: Spin {
                        pivot: Vec2::splat(50.0),
                        angle: FRAC_PI_2,
                    },
                    radius: Vec2::splat(50.0).length(),
                },
                origin: Vec2::ZERO,
                basis: CurveBasis::Arc {
                    center: Vec2::new(70.0, 50.0),
                    radius: 10.0,
                    a0: 0.0,
                    a1: PI,
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(!buf.curves.is_empty());
    for ci in &buf.curves {
        assert!(
            (ci.p0 - Vec2::new(50.0, 70.0)).length() < 1e-4,
            "center rotated about the bbox pivot, got {:?}",
            ci.p0,
        );
        assert!((ci.p2.x - FRAC_PI_2).abs() < 1e-6, "a0 offset by rotation");
        assert!((ci.p2.y - (PI + FRAC_PI_2)).abs() < 1e-6, "a1 offset");
    }
}

#[test]
fn compose_flat_cubic_emits_single_instance_curved_emits_many() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    // Same 800 px span: a straight cubic (CPs on the segment thirds —
    // exactly what Shape::line lowers to) must collapse to one
    // instance; a genuinely curved one must subdivide (800 px polygon
    // → ⌈⌈800/1.5⌉/16⌉ = 34 instances).
    let straight = |b: &mut PaintCapture| {
        b.draw_curve(DrawCurvePayload {
            bounds: StrokeBounds::Still(rect(0.0, 0.0, 800.0, 10.0)),
            origin: Vec2::ZERO,
            basis: CurveBasis::Cubic {
                p0: Vec2::new(0.0, 5.0),
                p1: Vec2::new(800.0 / 3.0, 5.0),
                p2: Vec2::new(1600.0 / 3.0, 5.0),
                p3: Vec2::new(800.0, 5.0),
            },
            color: Color::WHITE.into(),
            width: 2.0,
            ..Default::default()
        });
    };
    let curved = |b: &mut PaintCapture| {
        b.draw_curve(DrawCurvePayload {
            bounds: StrokeBounds::Still(rect(0.0, 0.0, 800.0, 400.0)),
            origin: Vec2::ZERO,
            basis: CurveBasis::Cubic {
                p0: Vec2::new(0.0, 5.0),
                p1: Vec2::new(266.0, 400.0),
                p2: Vec2::new(533.0, 400.0),
                p3: Vec2::new(800.0, 5.0),
            },
            color: Color::WHITE.into(),
            width: 2.0,
            ..Default::default()
        });
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

#[test]
fn compose_curve_spin_rotates_control_points_about_bbox_pivot() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    use std::f32::consts::FRAC_PI_2;
    // Pivot = bbox.center() = (50, 50). A π/2 spin (clockwise on
    // screen, y-down) maps an offset (dx, dy) from the pivot to
    // (-dy, dx). p0 = (70, 50) → (50, 70); p3 = (50, 30) → (70, 50).
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Spun {
                    spin: Spin {
                        pivot: Vec2::splat(50.0),
                        angle: FRAC_PI_2,
                    },
                    radius: Vec2::splat(50.0).length(),
                },
                origin: Vec2::ZERO,
                basis: CurveBasis::Cubic {
                    p0: Vec2::new(70.0, 50.0),
                    p1: Vec2::new(70.0, 40.0),
                    p2: Vec2::new(60.0, 30.0),
                    p3: Vec2::new(50.0, 30.0),
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(!buf.curves.is_empty());
    let ci = &buf.curves[0];
    assert!(
        (ci.p0 - Vec2::new(50.0, 70.0)).length() < 1e-4,
        "{:?}",
        ci.p0
    );
    assert!(
        (ci.p1 - Vec2::new(60.0, 70.0)).length() < 1e-4,
        "{:?}",
        ci.p1
    );
    assert!(
        (ci.p2 - Vec2::new(70.0, 60.0)).length() < 1e-4,
        "{:?}",
        ci.p2
    );
    assert!(
        (ci.p3 - Vec2::new(70.0, 50.0)).length() < 1e-4,
        "{:?}",
        ci.p3
    );
}

#[test]
fn compose_arc_and_curve_share_one_batch_per_group() {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::renderer::render_buffer::curve::{CURVE_KIND_ARC, CURVE_KIND_CUBIC};
    use crate::scene::shapes::paint::CurveBasis;
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(0.0, 0.0, 40.0, 40.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Arc {
                    center: Vec2::new(20.0, 20.0),
                    radius: 10.0,
                    a0: 0.0,
                    a1: 1.0,
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
            b.draw_curve(DrawCurvePayload {
                bounds: StrokeBounds::Still(rect(100.0, 0.0, 100.0, 100.0)),
                origin: Vec2::ZERO,
                basis: CurveBasis::Cubic {
                    p0: Vec2::new(100.0, 0.0),
                    p1: Vec2::new(110.0, 50.0),
                    p2: Vec2::new(190.0, 50.0),
                    p3: Vec2::new(200.0, 0.0),
                },
                color: Color::WHITE.into(),
                width: 2.0,
                ..Default::default()
            });
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(
        buf.batches(PaintTier::Curve).len(),
        1,
        "arcs batch with cubics"
    );
    assert_eq!(
        buf.batches(PaintTier::Curve)[0].items.len as usize,
        buf.curves.len()
    );
    assert!(buf.curves.iter().any(|c| c.kind == CURVE_KIND_ARC));
    assert!(buf.curves.iter().any(|c| c.kind == CURVE_KIND_CUBIC));
}

/// The backend replays a group's higher kinds in fixed tier order —
/// mesh batches → image batches → curve batches
/// (`schedule::emit_group_body`) — regardless of record order. A draw
/// recorded AFTER an overlapping draw of a later-replaying kind would
/// paint under it if both shared a group, so the composer must flush.
/// Record [curve, mesh]: one group would replay mesh→curve, inverting
/// record order → two groups (curve batch anchored at group 0, mesh
/// batch at group 1, restoring record order across groups).
#[test]
fn compose_curve_then_overlapping_mesh_splits_group() {
    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
            mesh(b, rect(10.0, 10.0, 30.0, 30.0)); // overlaps the curve bbox
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 2, "cross-kind conflict must split");
    assert_eq!(buf.batches(PaintTier::Curve).len(), 1);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Mesh).len(), 1);
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 1);
}

#[test]
fn two_point_polyline_does_not_reserve_miter_join_reach() {
    #[derive(Debug)]
    struct Case {
        points: &'static [Vec2],
        expected_groups: usize,
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
            expected_groups: 1,
        },
        Case {
            points: &THREE,
            expected_groups: 2,
        },
    ];

    for case in cases {
        let buf = run(
            |b, payloads| {
                polyline_cmd(
                    b,
                    payloads,
                    case.points,
                    &[Color::WHITE],
                    ColorMode::Single,
                    2.0,
                    LineCap::Butt,
                    LineJoin::Miter,
                );
                image(b, rect(22.0, 0.0, 10.0, 10.0));
            },
            &params(1.0, UVec2::new(100, 100)),
        );
        assert_eq!(buf.groups.len(), case.expected_groups, "{case:?}");
    }
}

/// `PaintSink::draw_polyline` *asserts* its no-op predicate instead of
/// gating on it, which is only safe if a degenerate polyline slipping
/// through in release degrades quietly rather than panicking or drawing
/// garbage. This is what makes that true, so it is the test to fix
/// before restoring the gate — not this one to delete.
///
/// Calls the **required** half directly, since that is the path below
/// the assert (and the one `PaintCapture::replay` takes).
#[test]
fn degenerate_polyline_emits_nothing_rather_than_panicking() {
    for points_len in [0u32, 1] {
        let buf = run(
            |b, arena| {
                arena.polyline_points.push(Vec2::ZERO);
                arena.polyline_colors.push(ColorU8::WHITE);
                b.polyline(DrawPolylinePayload {
                    bounds: StrokeBounds::Still(rect(0.0, 0.0, 4.0, 4.0)),
                    origin: Vec2::ZERO,
                    width: 2.0,
                    points_len,
                    colors_len: 1,
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
