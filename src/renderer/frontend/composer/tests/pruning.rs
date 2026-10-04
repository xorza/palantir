//! What the occlusion pass drops, and what it must not.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::lut_row::LutRow;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::composer::tests::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    clip, draw, draw_marked, params, params_unsnapped, run, survivor_calls, survivors, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use glam::UVec2;

#[test]
fn prune_drops_quad_fully_covered_by_later_opaque_quad() {
    // Two opaque quads over one rect: the first is fully covered by the
    // second, so prune drops it and the second survives.
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        survivor_calls(&buf),
        [1],
        "the covered first quad is pruned"
    );
}

#[test]
fn prune_non_fast_cover_insets_exact_half_pixel_aa_fringe() {
    #[derive(Debug)]
    struct Case {
        label: &'static str,
        under: Rect,
        expected_quads: usize,
    }

    let cases = [
        Case {
            label: "identical_fractional_edges",
            under: Rect::new(10.25, 10.25, 100.0, 100.0),
            expected_quads: 2,
        },
        Case {
            label: "touches_full_coverage_boundary",
            under: Rect::new(10.75, 10.75, 99.0, 99.0),
            expected_quads: 1,
        },
        Case {
            label: "crosses_full_coverage_boundary",
            under: Rect::new(10.74, 10.75, 99.0, 99.0),
            expected_quads: 2,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _| {
                draw(b, case.under);
                draw(b, Rect::new(10.25, 10.25, 100.0, 100.0));
            },
            &params_unsnapped(1.0, UVec2::new(200, 200)),
        );
        assert_eq!(buf.quads.len(), case.expected_quads, "{}", case.label);
    }
}

#[test]
fn prune_keeps_quad_not_fully_covered_by_smaller_later_quad() {
    // The on-top quad is smaller than the under quad — under survives.
    // `Rect::contains_rect` is asymmetric.
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            draw(b, Rect::new(10.0, 10.0, 50.0, 50.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
}

#[test]
fn prune_keeps_quads_in_separate_groups_even_when_covered() {
    // Group split: pushing a clip flushes; the later group's quad
    // can't reach back to prune the earlier group's quad.
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 2);
}

/// A quad's border is an inner-edge annulus, so it paints nothing
/// outside the rect: a bordered quad under an opaque cover of the same
/// rect is invisible and pruned, exactly like an unbordered one.
#[test]
fn prune_drops_bordered_quad_under_solid_cover() {
    use crate::primitives::paint::stroke::Stroke;

    let buf = run(
        |b, _| {
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .solid(RgbaF32::srgb(1.0, 0.0, 0.0))
                .stroke(Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 2.0))
                .draw(b);
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // solid on top
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    // The top quad is solid, opaque, sharp and pixel-aligned, so its
    // cover is its whole rect, which contains the bordered quad's rect.
    assert_eq!(buf.quads.len(), 1, "bordered under-quad pruned");
    assert_eq!(buf.quads[0].stroke_width, 0.0, "the cover survives");
}

#[test]
fn prune_rounded_on_top_uses_deflated_cover() {
    // Phase 3: a rounded-corner quad IS an occluder — but its
    // cover rect is its bounding rect deflated per side by the corner
    // inset plus the SDF's 0.5px AA transition. So a rounded occluder
    // strictly smaller (by the deflation margin)
    // than the under-quad does NOT fully cover it. Reversed: when
    // a sharp opaque quad on top exactly covers a rounded under,
    // the under is dropped (sharp cover == its own bounding rect,
    // which contains the rounded's bounding rect).

    let buf_rounded_on_top = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // solid sharp under
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .corners(Corners::all(10.0))
                .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf_rounded_on_top.quads.len(),
        2,
        "rounded occluder's deflated cover doesn't reach the under's edges",
    );

    let buf_sharp_on_top = run(
        |b, _| {
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .corners(Corners::all(10.0))
                .draw(b);
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // sharp opaque on top
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf_sharp_on_top.quads.len(),
        1,
        "rounded under-quad pruned when sharp opaque covers it",
    );
}

#[test]
fn prune_keeps_transparent_solid_as_non_occluder() {
    // alpha=0.5 quad on top doesn't occlude anything beneath.
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .solid(RgbaF32::srgba(1.0, 1.0, 1.0, 0.5))
                .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2, "semi-transparent does not occlude");
}

#[test]
fn prune_rounded_occluder_drops_smaller_under_inside_inscribed_rect() {
    // Phase 3: a rounded-corner opaque occluder fully covers a
    // sharp under-quad that fits entirely inside its inscribed
    // (KAPPA-deflated) rect. Rounded radius 10 plus the AA transition
    // gives a cover deflation of ≈3.43 per side.
    // An under-quad at (10,10,80,80) is well inside cover and
    // should be dropped.

    let buf = run(
        |b, _| {
            draw(b, Rect::new(10.0, 10.0, 80.0, 80.0)); // sharp opaque under
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .corners(Corners::all(10.0))
                .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        1,
        "rounded cover drops under fully inside inscribed rect"
    );
}

#[test]
fn prune_rounded_occluder_keeps_under_overlapping_corner_cutout() {
    // Phase 3: a sharp under-quad whose corner sits inside the
    // rounded occluder's corner cutout (the transparent triangle
    // between the bounding box corner and the arc's 45° point)
    // must NOT be dropped — the rounded paint doesn't reach there.
    // Rounded r=20 ⇒ inset ≈ 5.86. An under at (0,0,5,5) lies
    // entirely inside the [0,20]×[0,20] corner-cutout zone and is
    // never covered.

    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 5.0, 5.0)); // sharp under in corner
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .corners(Corners::all(20.0))
                .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        2,
        "under inside rounded cutout zone not dropped"
    );
}

#[test]
fn prune_keeps_shadow_under_opaque_cover() {
    use crate::primitives::packed::fill_axis::FillAxis;
    use crate::primitives::packed::fill_kind::FillKind;
    // A shadow's blur fringe extends past the stored rect — even if
    // a later opaque solid fully contains its rect, the visible
    // outer halo would be lost. Predicate must never drop shadows.
    let buf = run(
        |b, _| {
            b.draw_quad(
                DrawQuadPayload::shadow(
                    Rect::new(20.0, 20.0, 60.0, 60.0),
                    Corners::default(),
                    RgbaF32::srgba(0.0, 0.0, 0.0, 0.5).into(),
                    FillKind::SHADOW_DROP,
                    // (offset.x, offset.y, sigma, spread) — sigma=4 ⇒ 8-px halo.
                    FillAxis::from_lanes(0.0, 0.0, 4.0, 0.0),
                ),
                1.0,
            );
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // opaque cover on top
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        2,
        "shadow must survive even when its rect is fully contained",
    );
}

#[test]
fn prune_drops_chain_of_opaque_solids_keeping_only_topmost() {
    // Three identical opaque solids stacked. After prune only the
    // topmost survives. Walks back-to-front: A dropped by B (and by
    // C); B dropped by C; C survives. Exercises the "multiple
    // occluders per occludee" branch and verifies the compaction
    // logic handles two consecutive drops.
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // A
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // B
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // C
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(survivor_calls(&buf), [2], "only the topmost, C, survives");
}

#[test]
fn prune_stroked_occluder_drops_smaller_sharp_under() {
    // A solid-opaque occluder with a fully-OPAQUE stroke covers its
    // whole rect: quad_pipeline/shader.wgsl strokes are inner-edge and coverage-
    // partitioned with the fill, so opaque annulus + opaque fill =
    // opaque rect. A sharp under entirely inside should be dropped.
    // (Translucent strokes shrink the cover — see
    // `prune_occluder_stroke_translucency_gates_cover`.)
    use crate::primitives::paint::stroke::Stroke;

    let buf = run(
        |b, _| {
            draw(b, Rect::new(10.0, 10.0, 50.0, 50.0)); // sharp opaque under
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .stroke(Stroke::new(RgbaF32::srgb(0.0, 0.0, 0.0), 2.0))
                .draw(b);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        1,
        "opaque-stroked occluder still covers its rect",
    );
}

/// FIX-pin: quad_pipeline/shader.wgsl strokes are INNER-edge and coverage-partitioned
/// with the fill — the annulus's alpha is the stroke's alpha, not the
/// fill's. An opaque-fill quad is fully opaque only when its stroke is
/// a noop or fully opaque; a translucent stroke leaves a see-through
/// ring, so only the fill-only interior — the rect deflated by the
/// stroke width per side — may occlude.
///
/// Fixture: top quad = rect (0,0,100,100), sharp corners, opaque white
/// fill; stroke width 4 at scale 1. Hand-computed cover per case:
/// - noop stroke → fast path, cover = full rect (0,0)..(100,100).
/// - opaque stroke → cover = AA-deflated (0.5,0.5)..(99.5,99.5).
/// - 50%-alpha stroke → cover = deflated by 4.5/side.
/// - 50%-alpha stroke w=60 → deflation 60/side exceeds the 50 half-
///   extent → empty cover, no occluder recorded.
///
/// Bottom quad (a,b,c): same rect (0,0,100,100) — inside the full cover
/// but NOT inside (4,4)..(96,96). Bottom quad (d,e): (10,10,50,50) →
/// painted (10,10)..(60,60), inside (4,4)..(96,96).
#[test]
fn prune_occluder_stroke_translucency_gates_cover() {
    use crate::primitives::paint::stroke::Stroke;

    #[derive(Debug)]
    struct Case {
        label: &'static str,
        under: Rect,
        stroke: Stroke,
        pruned: bool,
    }
    let cases = [
        Case {
            label: "no_stroke_full_cover",
            under: Rect::new(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::NONE,
            pruned: true,
        },
        Case {
            label: "opaque_stroke_aa_edge_not_covered",
            under: Rect::new(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 4.0),
            pruned: false,
        },
        Case {
            label: "translucent_stroke_ring_not_covered",
            under: Rect::new(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 4.0),
            pruned: false,
        },
        Case {
            label: "translucent_stroke_interior_covered",
            under: Rect::new(10.0, 10.0, 50.0, 50.0),
            stroke: Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 4.0),
            pruned: true,
        },
        Case {
            label: "stroke_wider_than_half_rect_no_cover",
            under: Rect::new(10.0, 10.0, 50.0, 50.0),
            stroke: Stroke::new(RgbaF32::srgba(0.0, 1.0, 0.0, 0.5), 60.0),
            pruned: false,
        },
    ];
    for case in &cases {
        let buf = run(
            |b, _| {
                draw(b, case.under);
                QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                    .stroke(case.stroke)
                    .draw(b);
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        let expected = if case.pruned { 1 } else { 2 };
        assert_eq!(buf.quads.len(), expected, "case: {}", case.label);
    }
}

#[test]
fn prune_compacts_preserving_non_contiguous_survivors() {
    // Five quads, A to E. C covers A and E covers everything else, so
    // only E survives.
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 10.0, 10.0)); // A, inside C
            draw_marked(b, Rect::new(50.0, 50.0, 5.0, 5.0)); // B, inside E
            draw_marked(b, Rect::new(0.0, 0.0, 30.0, 30.0)); // C, covers A, inside E
            draw_marked(b, Rect::new(80.0, 80.0, 5.0, 5.0)); // D, inside E
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // E, covers the rest
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(survivor_calls(&buf), [4], "E covers everything");

    // With B and D outside every cover and no E, only A drops: the
    // survivors sit at indices 1, 2 and 3, and compaction keeps their
    // order.
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 10.0, 10.0)); // A, inside C
            draw_marked(b, Rect::new(150.0, 150.0, 5.0, 5.0)); // B
            draw_marked(b, Rect::new(0.0, 0.0, 30.0, 30.0)); // C
            draw_marked(b, Rect::new(170.0, 170.0, 5.0, 5.0)); // D
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(survivor_calls(&buf), [1, 2, 3]);
    assert_eq!(
        survivors(&buf),
        [
            Rect::new(150.0, 150.0, 5.0, 5.0),
            Rect::new(0.0, 0.0, 30.0, 30.0),
            Rect::new(170.0, 170.0, 5.0, 5.0),
        ],
    );
}

#[test]
fn prune_edge_tangent_under_is_dropped_under_inclusive_containment() {
    // The under-quad's max-edge equals the occluder's max-edge.
    // `Rect::contains_rect` is inclusive on equal edges, so the
    // under is fully contained and dropped. Pins the semantics —
    // a future "strict containment" tweak that flips this would
    // leak the tangent under-quad through.
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 50.0, 50.0)); // under, max=(50,50)
            draw(b, Rect::new(0.0, 0.0, 50.0, 50.0)); // occluder, same extent
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1, "identical extent → under dropped");
}

#[test]
fn prune_lower_index_occluder_does_not_drop_higher_index_under() {
    // Push a giant opaque solid FIRST, then a smaller under-quad on
    // top. The first quad would cover the second if order didn't
    // matter — but it's the under, not the occluder. Predicate must
    // respect paint order: only `occ.idx > i` qualifies.
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0)); // big "occluder" but painted FIRST
            draw(b, Rect::new(10.0, 10.0, 30.0, 30.0)); // small quad painted SECOND
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        2,
        "lower-idx occluder can't drop higher-idx under"
    );
}

#[test]
fn prune_steady_state_across_repeated_compose_calls() {
    // Run a pruning scenario five times against the same Composer
    // instance. Verifies scratch buffers (`opaque_in_group`,
    // `drop_indices`) are reset cleanly between frames — a stale
    // entry would either panic on index OOB after the slice shrinks
    // or leak across-frame drops.
    let mut buffer = PaintCapture::default();
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    for _ in 0..5 {
        buffer.calls.clear();
        draw(&mut buffer, Rect::new(0.0, 0.0, 100.0, 100.0));
        draw(&mut buffer, Rect::new(0.0, 0.0, 100.0, 100.0));
        rig.compose(&buffer);
        assert_eq!(rig.out.quads.len(), 1, "prune runs cleanly each frame");
    }
}

/// Clear fold: an opaque solid sharp unclipped quad covering the whole
/// viewport becomes `RenderBuffer::clear_override` instead of a quad, and
/// **discards everything composed before it** (a cover hides it all) —
/// while any disqualifier (corners, stroke, translucency, gradient,
/// partial coverage, active clip) leaves it as an ordinary quad over the
/// prior scene.
#[test]
fn clear_fold_absorbs_covers_and_rejects_non_qualifying() {
    type Build = fn(&mut PaintCapture);

    use crate::primitives::packed::fill_axis::FillAxis;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Spread;
    use crate::primitives::paint::color::rgba_f16::RgbaF16;

    let vp = UVec2::new(200, 200);
    let bg = RgbaF32::srgb(0.14, 0.16, 0.22);
    // The override rides a RgbaF16 lane; expected value is the f16
    // round-trip of the input, not the input itself.
    let folded = RgbaF16::from(bg).unpack();

    // (case, builder, expected quad count, expected override)
    let cases: &[(&str, Build, usize, Option<RgbaF32>)] = &[
        (
            "qualifying root folds, later quad stays",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 200.0, 200.0));
                draw(b, Rect::new(10.0, 10.0, 20.0, 20.0));
            },
            1,
            Some(RgbaF32::srgb(1.0, 1.0, 1.0)),
        ),
        (
            "rounded corners disqualify",
            |b| {
                QuadBuilder::new(Rect::new(0.0, 0.0, 200.0, 200.0))
                    .corners(Corners::all(4.0))
                    .draw(b);
            },
            1,
            None,
        ),
        (
            "stroke disqualifies",
            |b| {
                QuadBuilder::new(Rect::new(0.0, 0.0, 200.0, 200.0))
                    .stroke(Stroke::new(RgbaF32::WHITE, 2.0))
                    .draw(b);
            },
            1,
            None,
        ),
        (
            "translucent fill disqualifies",
            |b| {
                QuadBuilder::new(Rect::new(0.0, 0.0, 200.0, 200.0))
                    .solid(RgbaF32::srgba(1.0, 1.0, 1.0, 0.5))
                    .draw(b);
            },
            1,
            None,
        ),
        (
            "gradient fill disqualifies",
            |b| {
                QuadBuilder::new(Rect::new(0.0, 0.0, 200.0, 200.0))
                    .brush(BrushSource::Gradient(ResolvedGradient {
                        axis: FillAxis::ZERO,
                        lut_row: LutRow::FALLBACK,
                        kind: FillKind::linear(Spread::Pad),
                    }))
                    .draw(b);
            },
            1,
            None,
        ),
        (
            "one pixel short of coverage disqualifies",
            |b| draw(b, Rect::new(0.0, 0.0, 200.0, 199.0)),
            1,
            None,
        ),
        (
            "prior quad is discarded under a later cover",
            |b| {
                // Straddles the viewport edge so it isn't the per-group
                // occlusion pruner doing the work — the fold's discard
                // must drop it.
                draw(b, Rect::new(-10.0, -10.0, 20.0, 20.0));
                draw(b, Rect::new(0.0, 0.0, 200.0, 200.0));
            },
            0,
            Some(RgbaF32::srgb(1.0, 1.0, 1.0)),
        ),
        (
            "active clip disqualifies",
            |b| {
                clip(b, Rect::new(0.0, 0.0, 150.0, 150.0));
                draw(b, Rect::new(0.0, 0.0, 200.0, 200.0));
                b.pop_clip();
            },
            1,
            None,
        ),
        (
            "second qualifying cover re-folds over the first",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 200.0, 200.0));
                QuadBuilder::new(Rect::new(0.0, 0.0, 200.0, 200.0))
                    .solid(RgbaF32::srgb(0.14, 0.16, 0.22))
                    .draw(b);
            },
            0,
            Some(RgbaF32::srgb(0.14, 0.16, 0.22)),
        ),
    ];

    for (name, build, want_quads, want_override) in cases {
        let buf = run(|b, _arena| build(b), &params(1.0, vp));
        assert_eq!(
            buf.quads.len(),
            *want_quads,
            "{name}: quad count after fold decision",
        );
        let want = want_override.map(|c| RgbaF16::from(c).unpack());
        assert_eq!(buf.clear_override, want, "{name}: clear_override");
    }

    // Coverage in physical px: a logical half-viewport rect at DPR 2
    // covers the full physical viewport and folds.
    let buf = run(
        |b, _arena| {
            QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
                .solid(bg)
                .draw(b);
        },
        &params(2.0, vp),
    );
    assert_eq!(buf.quads.len(), 0, "DPR-2 cover folds");
    assert_eq!(buf.clear_override, Some(folded), "DPR-2 override color");
}

/// A mid-stream cover discards the whole hidden underlay — text runs,
/// clipped groups and their batches — not just quads; content recorded
/// after the cover composes normally, and a transform in flight when the
/// cover lands survives the discard (its pops are still ahead).
#[test]
fn clear_fold_discards_hidden_underlay_mid_stream() {
    use crate::primitives::paint::color::rgba_f16::RgbaF16;

    let vp = UVec2::new(200, 200);

    let buf = run(
        |b, _arena| {
            // Hidden underlay: a text run and a quad inside a clipped group.
            text(b, Rect::new(10.0, 10.0, 50.0, 20.0));
            clip(b, Rect::new(0.0, 0.0, 150.0, 150.0));
            draw(b, Rect::new(10.0, 10.0, 20.0, 20.0));
            b.pop_clip();
            // The cover lands under an active 2x transform: its world rect
            // (0,0)-(200,200) covers the viewport, so it folds — and the
            // transform must keep applying to the survivor below.
            b.push_transform(TranslateScale::from_scale(2.0));
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            draw(b, Rect::new(5.0, 5.0, 10.0, 10.0));
            b.pop_transform();
            text(b, Rect::new(30.0, 30.0, 40.0, 10.0));
        },
        &params(1.0, vp),
    );

    let folded = RgbaF16::from(RgbaF32::srgb(1.0, 1.0, 1.0)).unpack();
    assert_eq!(buf.clear_override, Some(folded), "the cover folds");
    // Underlay gone: only the post-cover quad + text survive, in one
    // unscissored group (the pre-cover clipped group was discarded).
    assert_eq!(buf.quads.len(), 1, "underlay quads discarded");
    assert_eq!(
        buf.quads[0].rect,
        Rect::new(10.0, 10.0, 20.0, 20.0),
        "survivor keeps the in-flight 2x transform",
    );
    assert_eq!(buf.texts.len(), 1, "underlay text discarded");
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_none());
}

/// `clear_override` is per-frame state: a fold one frame must not leak
/// into the next frame's buffer when the cover disappears, and a
/// steady-state cover re-folds every frame.
#[test]
fn clear_fold_resets_across_frames() {
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));

    let mut covered = PaintCapture::default();
    draw(&mut covered, Rect::new(0.0, 0.0, 200.0, 200.0));
    draw(&mut covered, Rect::new(10.0, 10.0, 20.0, 20.0));

    rig.compose(&covered);
    assert!(rig.out.clear_override.is_some(), "frame 1 folds");
    assert_eq!(rig.out.quads.len(), 1);

    rig.compose(&covered);
    assert!(rig.out.clear_override.is_some(), "steady state re-folds");
    assert_eq!(rig.out.quads.len(), 1);

    let mut uncovered = PaintCapture::default();
    draw(&mut uncovered, Rect::new(10.0, 10.0, 20.0, 20.0));
    rig.compose(&uncovered);
    assert_eq!(rig.out.clear_override, None, "no cover, no override");
    assert_eq!(rig.out.quads.len(), 1);
}

/// A pixel-aligned opaque quad's cover is its own rect, so quad `i`
/// survives exactly when no later quad's rect contains it.
fn brute_force_survivors(rects: &[Rect]) -> Vec<Rect> {
    rects
        .iter()
        .enumerate()
        .filter(|&(i, r)| !rects[i + 1..].iter().any(|later| later.contains_rect(*r)))
        .map(|(_, r)| *r)
        .collect()
}

/// The indexed prune drops exactly what a scan of every later cover
/// drops. 400 pseudo-random pixel-aligned rects over a 1280 × 1280
/// viewport of 20 × 20 tiles: small ones, ones spanning many tiles,
/// ones past [`LARGE_COVER_TILES`]'s 256 tiles (from 1040 px a side,
/// 17 × 17 tiles), ones reaching past the viewport, and repeats of
/// earlier rects so that many quads are covered.
#[test]
fn indexed_prune_matches_a_full_scan() {
    let mut seed = 0x2545_f491_u32;
    let mut next = |bound: u32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) % bound
    };
    let mut rects = Vec::new();
    for i in 0..400 {
        let r = match i % 5 {
            0 if i > 0 => rects[next(i) as usize],
            1 => Rect::new(
                next(1200) as f32,
                next(1200) as f32,
                (next(40) + 1) as f32,
                (next(40) + 1) as f32,
            ),
            2 => Rect::new(
                next(600) as f32,
                next(600) as f32,
                (next(500) + 1) as f32,
                (next(500) + 1) as f32,
            ),
            3 => Rect::new(
                next(200) as f32,
                next(200) as f32,
                (1040 + next(400)) as f32,
                (1040 + next(400)) as f32,
            ),
            _ => Rect::new(
                next(1280) as f32,
                next(1280) as f32,
                (next(900) + 1) as f32,
                (next(900) + 1) as f32,
            ),
        };
        rects.push(r);
    }
    let expected = brute_force_survivors(&rects);
    assert!(
        expected.len() < rects.len() - 50,
        "premise: the fixture covers many quads ({} of {} survive)",
        expected.len(),
        rects.len(),
    );
    let buf = run(
        |b, _| {
            for r in &rects {
                draw(b, *r);
            }
        },
        &params(1.0, UVec2::new(1280, 1280)),
    );
    let survivors: Vec<Rect> = buf.quads.iter().map(|q| q.rect).collect();
    assert_eq!(survivors, expected);
}

/// The prune's cost is local. A 96 × 96 grid of 8 px cells, one opaque
/// quad each, fills a 768 × 768 viewport of 12 × 12 tiles with 64 cells
/// per tile. None covers another. A cell is tested only against the
/// later cells of its own tile: the cells of a tile come in row-major
/// order, so its `k`th cell is tested against `63 - k` of them, and a
/// tile costs `0 + 1 + … + 63 = 2016` tests. A scan of every later cover
/// costs `9216 × 9215 / 2 = 42 462 720`.
#[test]
fn equal_cells_cost_one_tile_each() {
    let mut recorded = PaintCapture::default();
    for row in 0..96 {
        for column in 0..96 {
            draw(
                &mut recorded,
                Rect::new(column as f32 * 8.0, row as f32 * 8.0, 8.0, 8.0),
            );
        }
    }
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(768, 768)));
    rig.compose(&recorded);
    assert_eq!(rig.out.quads.len(), 96 * 96, "no cell covers another");
    assert_eq!(rig.composer.occlusion.contains_tests(), 144 * 2016);
}
