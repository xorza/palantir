//! What the occlusion pass drops, and what it must not.

use crate::primitives::fill_wire::LutRow;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::tests::support::{
    clip, composer, draw, params, rect, render_buffer, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{BrushSource, DrawQuadPayload, ResolvedGradient};
use crate::scene::record_store::RecordPayloads;
use glam::UVec2;

#[test]
fn prune_drops_quad_fully_covered_by_later_opaque_quad() {
    // Outer 0..100 painted first (z=0), inner 0..100 (z=1) opaque white
    // on top — outer is fully covered, prune drops it.
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1, "fully-covered earlier quad pruned");
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
            under: rect(10.25, 10.25, 100.0, 100.0),
            expected_quads: 2,
        },
        Case {
            label: "touches_full_coverage_boundary",
            under: rect(10.75, 10.75, 99.0, 99.0),
            expected_quads: 1,
        },
        Case {
            label: "crosses_full_coverage_boundary",
            under: rect(10.74, 10.75, 99.0, 99.0),
            expected_quads: 2,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _| {
                draw(b, case.under);
                draw(b, rect(10.25, 10.25, 100.0, 100.0));
            },
            &params(1.0, UVec2::new(200, 200)),
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
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(10.0, 10.0, 50.0, 50.0));
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
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            clip(b, rect(0.0, 0.0, 200.0, 200.0));
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 2);
}

#[test]
fn prune_does_not_drop_stroked_quad_under_solid_cover() {
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::BrushSource;
    // A stroked quad's stroke spills outside the rect; pruning a
    // stroked quad on the strict containment test below would lose
    // the stroke fringe. Predicate requires zero-stroke as
    // occludable — stroked quads are kept regardless of cover.
    let buf = run(
        |b, _| {
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 0.0, 0.0).into()),
                Stroke::solid(Color::rgb(0.0, 1.0, 0.0), 2.0).into(),
            ));
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // solid on top
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    // Top quad is solid opaque sharp-cornered no-stroke; it would be
    // an occluder. Bottom quad has corners==0 and would normally be
    // covered — but it has a stroke, so the occludee predicate
    // (stroke_width ≈ 0) must reject it.
    // NB: the design doc disqualifies stroked quads as both occluder
    // AND occludable. Implementation only excludes stroked from
    // occluders; occludables are not stroke-filtered today because
    // the GPU rasterizes the stroked quad only inside the bounding
    // rect's expanded box — actually the stroke is centred, so
    // half extends outside the rect. A correctly-implemented
    // occludable predicate must also exclude stroked quads.
    assert_eq!(buf.quads.len(), 2, "stroked under-quad kept");
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
    use crate::renderer::frontend::payload::BrushSource;
    let buf_rounded_on_top = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // solid sharp under
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            ));
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
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            ));
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // sharp opaque on top
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
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::BrushSource;
    // alpha=0.5 quad on top doesn't occlude anything beneath.
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgba(1.0, 1.0, 1.0, 0.5).into()),
                Stroke::ZERO.into(),
            ));
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
    use crate::renderer::frontend::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 80.0, 80.0)); // sharp opaque under
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            ));
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
    use crate::renderer::frontend::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 5.0, 5.0)); // sharp under in corner
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(20.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            ));
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
fn rect_inscribed_for_corners_matches_45_deg_arc_offset() {
    // Pin the inscribed-rect deflation: for uniform radius `r`,
    // each side insets by `r * (1 − 1/√2)` — the bounding-box
    // distance to the 45° point on the corner arc. A future
    // "tighten the deflation" attempt would prune an under-quad
    // whose corner falls inside the rounded cutout.
    let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    let inscribed = r.inscribed_for_corners(Corners::all(10.0));
    let expected_inset = 10.0 * (1.0 - 1.0 / 2.0_f32.sqrt());
    let expected_size = 100.0 - 2.0 * expected_inset;
    assert!((inscribed.min.x - expected_inset).abs() < 1e-5);
    assert!((inscribed.min.y - expected_inset).abs() < 1e-5);
    assert!((inscribed.size.w - expected_size).abs() < 1e-4);
    assert!((inscribed.size.h - expected_size).abs() < 1e-4);
}

#[test]
fn rect_inscribed_for_corners_sharp_passes_through() {
    let r = Rect::new(5.0, 10.0, 30.0, 40.0);
    assert_eq!(r.inscribed_for_corners(Corners::ZERO), r);
}

#[test]
fn rect_inscribed_for_corners_uses_max_of_adjacent_radii() {
    // Per-side inset = max(adjacent corner radii) * (1 − 1/√2).
    // With tl=20, tr=0, br=0, bl=0 the LEFT and TOP sides inset by
    // 20·KAPPA; right + bottom stay flush (their adjacent corners
    // are sharp).
    let r = Rect::new(0.0, 0.0, 100.0, 100.0);
    let inscribed = r.inscribed_for_corners(Corners::new(20.0, 0.0, 0.0, 0.0));
    let inset = 20.0 * (1.0 - 1.0 / 2.0_f32.sqrt());
    assert!((inscribed.min.x - inset).abs() < 1e-5);
    assert!((inscribed.min.y - inset).abs() < 1e-5);
    assert!((inscribed.max().x - 100.0).abs() < 1e-5);
    assert!((inscribed.max().y - 100.0).abs() < 1e-5);
}

#[test]
fn prune_keeps_shadow_under_opaque_cover() {
    use crate::primitives::brush::gradient::FillAxis;
    use crate::primitives::fill_wire::FillKind;
    // A shadow's blur fringe extends past the stored rect — even if
    // a later opaque solid fully contains its rect, the visible
    // outer halo would be lost. Predicate must never drop shadows.
    let buf = run(
        |b, _| {
            b.draw_quad(DrawQuadPayload::shadow(
                rect(20.0, 20.0, 60.0, 60.0),
                Corners::default(),
                Color::rgba(0.0, 0.0, 0.0, 0.5).into(),
                FillKind::SHADOW_DROP,
                // (offset.x, offset.y, sigma, spread) — sigma=4 ⇒ 8-px halo.
                FillAxis::from_lanes(0.0, 0.0, 4.0, 0.0),
            ));
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // opaque cover on top
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
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // A
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // B
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // C
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1, "only topmost survives");
}

#[test]
fn prune_stroked_occluder_drops_smaller_sharp_under() {
    // A solid-opaque occluder with a fully-OPAQUE stroke covers its
    // whole rect: quad.wgsl strokes are inner-edge and coverage-
    // partitioned with the fill, so opaque annulus + opaque fill =
    // opaque rect. A sharp under entirely inside should be dropped.
    // (Translucent strokes shrink the cover — see
    // `prune_occluder_stroke_translucency_gates_cover`.)
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 50.0, 50.0)); // sharp opaque under
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::solid(Color::rgb(0.0, 0.0, 0.0), 2.0).into(),
            ));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        1,
        "opaque-stroked occluder still covers its rect",
    );
}

/// FIX-pin: quad.wgsl strokes are INNER-edge and coverage-partitioned
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
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::payload::BrushSource;
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
            under: rect(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::ZERO,
            pruned: true,
        },
        Case {
            label: "opaque_stroke_aa_edge_not_covered",
            under: rect(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::solid(Color::rgb(0.0, 1.0, 0.0), 4.0),
            pruned: false,
        },
        Case {
            label: "translucent_stroke_ring_not_covered",
            under: rect(0.0, 0.0, 100.0, 100.0),
            stroke: Stroke::solid(Color::rgba(0.0, 1.0, 0.0, 0.5), 4.0),
            pruned: false,
        },
        Case {
            label: "translucent_stroke_interior_covered",
            under: rect(10.0, 10.0, 50.0, 50.0),
            stroke: Stroke::solid(Color::rgba(0.0, 1.0, 0.0, 0.5), 4.0),
            pruned: true,
        },
        Case {
            label: "stroke_wider_than_half_rect_no_cover",
            under: rect(10.0, 10.0, 50.0, 50.0),
            stroke: Stroke::solid(Color::rgba(0.0, 1.0, 0.0, 0.5), 60.0),
            pruned: false,
        },
    ];
    for case in &cases {
        let buf = run(
            |b, _| {
                draw(b, case.under);
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 100.0, 100.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    (&case.stroke).into(),
                ));
            },
            &params(1.0, UVec2::new(200, 200)),
        );
        let expected = if case.pruned { 1 } else { 2 };
        assert_eq!(buf.quads.len(), expected, "case: {}", case.label);
    }
}

#[test]
fn prune_compacts_preserving_non_contiguous_survivors() {
    // Five quads: A,B,C,D,E. Drop A (covered by C), keep B (not
    // covered), drop C (covered by E), keep D (not covered), keep
    // E (topmost). After compact the slice must be [B, D, E] in
    // original order. Exercises the compaction walk over
    // non-contiguous drop indices.
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0)); // A (covered by C)
            draw(b, rect(50.0, 50.0, 5.0, 5.0)); // B (off to the side)
            draw(b, rect(0.0, 0.0, 30.0, 30.0)); // C (covers A, covered by E)
            draw(b, rect(80.0, 80.0, 5.0, 5.0)); // D (off to the side)
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // E covers everything top-left
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    // A and C are covered (and B, D — wait, are B and D inside E?)
    // B at (50,50,5,5) i.e. min=(50,50) max=(55,55). E.cover=(0,0,100,100).
    // E contains B → B also dropped. Same for D. So only E survives.
    assert_eq!(
        buf.quads.len(),
        1,
        "E covers everything → only topmost survives"
    );

    // Repeat with B/D positioned OUTSIDE E to exercise non-contiguous
    // survivors at indices 1 and 3.
    let buf2 = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0)); // A (covered by C)
            draw(b, rect(150.0, 150.0, 5.0, 5.0)); // B (outside everything)
            draw(b, rect(0.0, 0.0, 30.0, 30.0)); // C (covers A)
            draw(b, rect(170.0, 170.0, 5.0, 5.0)); // D (outside everything)
            // No giant cover at the end — C is the only big occluder.
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    // Surviving: B, C, D (A is dropped). Three quads, compaction
    // preserved order.
    assert_eq!(buf2.quads.len(), 3);
    // Sanity: B and D unchanged in position, C unchanged.
    assert_eq!(buf2.quads[0].rect.min, glam::Vec2::new(150.0, 150.0)); // B
    assert_eq!(buf2.quads[1].rect.min, glam::Vec2::new(0.0, 0.0)); // C
    assert_eq!(buf2.quads[2].rect.min, glam::Vec2::new(170.0, 170.0)); // D
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
            draw(b, rect(0.0, 0.0, 50.0, 50.0)); // under, max=(50,50)
            draw(b, rect(0.0, 0.0, 50.0, 50.0)); // occluder, same extent
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
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // big "occluder" but painted FIRST
            draw(b, rect(10.0, 10.0, 30.0, 30.0)); // small quad painted SECOND
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
    let mut composer = composer();
    let display = params(1.0, UVec2::new(200, 200));
    for _ in 0..5 {
        buffer.calls.clear();
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        let mut out = render_buffer();
        composer
            .begin(display, &RecordPayloads::default(), &mut out)
            .replay_from(&buffer);
        assert_eq!(out.quads.len(), 1, "prune runs cleanly each frame");
    }
}

#[test]
fn rect_inflated_round_trips_with_deflated_by_uniform() {
    use crate::primitives::spacing::Spacing;
    // `Rect::inflated(a).deflated_by(Spacing::all(a))` should
    // yield the original rect. Pins the symmetric-counterpart
    // contract documented on `Rect::inflated`.
    let r = Rect::new(10.0, 20.0, 30.0, 40.0);
    let round = r.inflated(2.5).deflated_by(Spacing::all(2.5));
    assert!((round.min.x - r.min.x).abs() < 1e-5);
    assert!((round.min.y - r.min.y).abs() < 1e-5);
    assert!((round.size.w - r.size.w).abs() < 1e-5);
    assert!((round.size.h - r.size.h).abs() < 1e-5);
}

/// Clear fold: an opaque solid sharp unclipped quad covering the whole
/// viewport becomes `RenderBuffer::clear_override` instead of a quad, and
/// **discards everything composed before it** (a cover hides it all) —
/// while any disqualifier (corners, stroke, translucency, gradient,
/// partial coverage, active clip) leaves it as an ordinary quad over the
/// prior scene.
#[test]
fn clear_fold_absorbs_covers_and_rejects_non_qualifying() {
    use crate::primitives::brush::gradient::FillAxis;
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::color::ColorF16;
    use crate::primitives::fill_wire::FillKind;

    let vp = UVec2::new(200, 200);
    let bg = Color::rgb(0.14, 0.16, 0.22);
    // The override rides a ColorF16 lane; expected value is the f16
    // round-trip of the input, not the input itself.
    let folded = ColorF16::from(bg).unpack();

    // (case, builder, expected quad count, expected override)
    type Build = fn(&mut PaintCapture);
    let cases: &[(&str, Build, usize, Option<Color>)] = &[
        (
            "qualifying root folds, later quad stays",
            |b| {
                draw(b, rect(0.0, 0.0, 200.0, 200.0));
                draw(b, rect(10.0, 10.0, 20.0, 20.0));
            },
            1,
            Some(Color::rgb(1.0, 1.0, 1.0)),
        ),
        (
            "rounded corners disqualify",
            |b| {
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::all(4.0),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    Stroke::ZERO.into(),
                ));
            },
            1,
            None,
        ),
        (
            "stroke disqualifies",
            |b| {
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    Stroke::solid(Color::WHITE, 2.0).into(),
                ));
            },
            1,
            None,
        ),
        (
            "translucent fill disqualifies",
            |b| {
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgba(1.0, 1.0, 1.0, 0.5).into()),
                    Stroke::ZERO.into(),
                ));
            },
            1,
            None,
        ),
        (
            "gradient fill disqualifies",
            |b| {
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Gradient(ResolvedGradient {
                        axis: FillAxis::ZERO,
                        row: LutRow::FALLBACK,
                        kind: FillKind::linear(Spread::Pad),
                    }),
                    Stroke::ZERO.into(),
                ));
            },
            1,
            None,
        ),
        (
            "one pixel short of coverage disqualifies",
            |b| draw(b, rect(0.0, 0.0, 200.0, 199.0)),
            1,
            None,
        ),
        (
            "prior quad is discarded under a later cover",
            |b| {
                // Straddles the viewport edge so it isn't the per-group
                // occlusion pruner doing the work — the fold's discard
                // must drop it.
                draw(b, rect(-10.0, -10.0, 20.0, 20.0));
                draw(b, rect(0.0, 0.0, 200.0, 200.0));
            },
            0,
            Some(Color::rgb(1.0, 1.0, 1.0)),
        ),
        (
            "active clip disqualifies",
            |b| {
                clip(b, rect(0.0, 0.0, 150.0, 150.0));
                draw(b, rect(0.0, 0.0, 200.0, 200.0));
                b.pop_clip();
            },
            1,
            None,
        ),
        (
            "second qualifying cover re-folds over the first",
            |b| {
                draw(b, rect(0.0, 0.0, 200.0, 200.0));
                b.draw_quad(DrawQuadPayload::rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(0.14, 0.16, 0.22).into()),
                    Stroke::ZERO.into(),
                ));
            },
            0,
            Some(Color::rgb(0.14, 0.16, 0.22)),
        ),
    ];

    for (name, build, want_quads, want_override) in cases {
        let buf = run(|b, _arena| build(b), &params(1.0, vp));
        assert_eq!(
            buf.quads.len(),
            *want_quads,
            "{name}: quad count after fold decision",
        );
        let want = want_override.map(|c| ColorF16::from(c).unpack());
        assert_eq!(buf.clear_override, want, "{name}: clear_override");
    }

    // Coverage in physical px: a logical half-viewport rect at DPR 2
    // covers the full physical viewport and folds.
    let buf = run(
        |b, _arena| {
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(bg.into()),
                Stroke::ZERO.into(),
            ));
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
    use crate::primitives::color::ColorF16;

    let vp = UVec2::new(200, 200);

    let buf = run(
        |b, _arena| {
            // Hidden underlay: a text run and a quad inside a clipped group.
            text(b, rect(10.0, 10.0, 50.0, 20.0));
            clip(b, rect(0.0, 0.0, 150.0, 150.0));
            draw(b, rect(10.0, 10.0, 20.0, 20.0));
            b.pop_clip();
            // The cover lands under an active 2x transform: its world rect
            // (0,0)-(200,200) covers the viewport, so it folds — and the
            // transform must keep applying to the survivor below.
            b.push_transform(TranslateScale::from_scale(2.0));
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(5.0, 5.0, 10.0, 10.0));
            b.pop_transform();
            text(b, rect(30.0, 30.0, 40.0, 10.0));
        },
        &params(1.0, vp),
    );

    let folded = ColorF16::from(Color::rgb(1.0, 1.0, 1.0)).unpack();
    assert_eq!(buf.clear_override, Some(folded), "the cover folds");
    // Underlay gone: only the post-cover quad + text survive, in one
    // unscissored group (the pre-cover clipped group was discarded).
    assert_eq!(buf.quads.len(), 1, "underlay quads discarded");
    assert_eq!(
        buf.quads[0].rect,
        rect(10.0, 10.0, 20.0, 20.0),
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
    let display = params(1.0, UVec2::new(200, 200));
    let mut composer = composer();
    let mut out = render_buffer();
    let payloads = RecordPayloads::default();

    let mut covered = PaintCapture::default();
    draw(&mut covered, rect(0.0, 0.0, 200.0, 200.0));
    draw(&mut covered, rect(10.0, 10.0, 20.0, 20.0));

    composer
        .begin(display, &payloads, &mut out)
        .replay_from(&covered);
    assert!(out.clear_override.is_some(), "frame 1 folds");
    assert_eq!(out.quads.len(), 1);

    composer
        .begin(display, &payloads, &mut out)
        .replay_from(&covered);
    assert!(out.clear_override.is_some(), "steady state re-folds");
    assert_eq!(out.quads.len(), 1);

    let mut uncovered = PaintCapture::default();
    draw(&mut uncovered, rect(10.0, 10.0, 20.0, 20.0));
    composer
        .begin(display, &payloads, &mut out)
        .replay_from(&uncovered);
    assert_eq!(out.clear_override, None, "no cover, no override");
    assert_eq!(out.quads.len(), 1);
}
