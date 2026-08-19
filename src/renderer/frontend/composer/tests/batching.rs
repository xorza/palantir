//! Which draws share a group and a batch, and what forces a split.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::span::Span;
use crate::primitives::{color::Color, corners::Corners, rect::Rect, stroke::Stroke, urect::URect};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::tests::support::{
    clip, clip_rounded, composer, curve, draw, image, mesh, params, polyline_cmd, rect,
    render_buffer, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    BrushSource, DrawPolylinePayload, DrawQuadPayload, ResolvedGradient, Spin, StrokeBounds,
};
use crate::renderer::render_buffer::batch::PaintTier;
use crate::scene::record_store::RecordPayloads;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::{UVec2, Vec2};
use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

/// Pin: a `Quad → Text → Quad` paint sequence inside a single scissor
/// produces TWO groups so the second quad renders *after* the text.
/// Without this split, `submit` batches both quads together and the
/// text always paints on top — which is the bug the `text z-order`
/// showcase tab exposes.
#[test]
fn compose_splits_group_on_text_to_quad_transition() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
            draw(b, rect(20.0, 20.0, 60.0, 40.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "text→quad transition must start a new group"
    );
    // First group: quad #0; the text rides its batch, anchored at
    // group 0 so it renders after that group's quad.
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 1));
    assert_eq!(buf.text_batches[0].last_group, 0);
    // Second group: quad #1 only — renders after group 0's text.
    assert_eq!(buf.groups[1].quads, Span::new(1, 1));
}

/// Pin: consecutive `Text → Text` should NOT split (both go into the
/// same group). Only `Text → Quad` triggers a flush. Otherwise a
/// header-then-body label pair produces two groups for nothing.
#[test]
fn compose_does_not_split_consecutive_texts() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
            text(b, rect(10.0, 35.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    // Both runs coalesce into one batch anchored at the single group.
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 2));
    assert_eq!(buf.text_batches[0].last_group, 0);
}

/// Pin: a nested clip that resolves to the same scissor as its
/// parent (a redundant `PushClip` of an equal-or-larger rect) is a
/// no-op — accumulated overlap state must survive the push/pop pair
/// so a later disjoint quad still batches into the open group.
/// Without this, anything emitted between the inner Push and Pop
/// would lose the parent's text-overlap context and a following
/// quad could reorder over earlier text.
#[test]
fn compose_same_clip_push_pop_preserves_overlap_state() {
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 200.0, 200.0));
            draw(b, rect(0.0, 0.0, 100.0, 28.0)); // node A bg
            text(b, rect(4.0, 4.0, 90.0, 20.0)); //  node A label
            // Redundant nested clip — same rect, no narrowing.
            clip(b, rect(0.0, 0.0, 200.0, 200.0));
            b.pop_clip();
            // Overlapping bg after the redundant clip: must still
            // flush against node A's label.
            draw(b, rect(40.0, 10.0, 100.0, 28.0)); // node B bg, overlaps A's label
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "overlap state must survive a redundant clip Push/Pop",
    );
}

/// Pin: a stack of `(quad, text)` row units that don't overlap each
/// other batches into ONE group. This is the row-list / grid case —
/// 40 rows each with a background and a label should collapse to a
/// single `quads` batch and a single `texts` batch, not 40 groups.
/// Overlap-aware composer: a later quad only flushes when it
/// intersects a prior text in the same group; disjoint rows stay
/// batched.
#[test]
fn compose_batches_disjoint_row_units_into_one_group() {
    let buf = run(
        |b, _arena| {
            for i in 0..5 {
                let y = (i as f32) * 40.0;
                draw(b, rect(0.0, y, 100.0, 28.0));
                text(b, rect(4.0, y + 4.0, 90.0, 20.0));
            }
        },
        &params(1.0, UVec2::new(200, 400)),
    );
    assert_eq!(buf.quads.len(), 5);
    assert_eq!(buf.texts.len(), 5);
    assert_eq!(
        buf.groups.len(),
        1,
        "disjoint (quad,text) rows must batch into one group",
    );
    assert_eq!(buf.groups[0].quads, Span::new(0, 5));
    assert_eq!(buf.text_batches.len(), 1, "one texts batch for all rows");
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 5));
}

/// Pin: when a later quad DOES overlap a prior text (the node-editor
/// case — node B's chrome lands on node A's label), the composer
/// must flush so paint order is preserved. Same fixture shape as the
/// row-batching test but the second row's chrome is offset to land
/// on the first row's label.
#[test]
fn compose_flushes_when_later_quad_overlaps_prior_text() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(0.0, 0.0, 100.0, 28.0)); // node A chrome
            text(b, rect(4.0, 4.0, 90.0, 20.0)); //  node A label
            draw(b, rect(40.0, 10.0, 100.0, 28.0)); // node B chrome, overlaps A's label
            text(b, rect(44.0, 14.0, 90.0, 20.0)); // node B label
        },
        &params(1.0, UVec2::new(400, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(
        buf.groups.len(),
        2,
        "overlapping quad-after-text must start a new group",
    );
}

#[test]
fn compose_shadow_outer_halo_after_text_splits_group() {
    let sigma = 4.0;
    let source = rect(50.0, 50.0, 50.0, 50.0);
    let shadow_rect = source.inflated(3.0 * sigma);
    let buf = run(
        |b, _arena| {
            text(b, rect(39.0, 60.0, 2.0, 10.0));
            b.draw_quad(DrawQuadPayload::shadow(
                shadow_rect,
                Corners::ZERO,
                Color::BLACK.into(),
                FillKind::SHADOW_DROP,
                FillAxis::from_lanes(0.0, 0.0, sigma, 0.0),
            ));
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    assert_eq!(buf.groups.len(), 2, "outer halo overlap must split");
    assert_eq!(buf.text_batches[0].last_group, 0);
    assert_eq!(buf.groups[1].quads, Span::new(0, 1));
}

/// Pin: `Quad → Quad → Text` fits in one group. The text comes after
/// both quads and renders on top of both — the common case (button
/// background + button stroke + label).
#[test]
fn compose_keeps_quads_then_text_in_one_group() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(2.0, 2.0, 96.0, 96.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 1));
    assert_eq!(buf.text_batches[0].last_group, 0);
}

/// Pin: two adjacent rows where each row sits in its own scissor
/// (a clipped panel per row) coalesce their text into ONE batch even
/// though they're in different groups. Saves a glyphon prepare +
/// render per extra row — the bulk of the savings from text batching.
#[test]
fn compose_coalesces_text_across_distinct_scissor_groups() {
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 30.0));
            draw(b, rect(0.0, 0.0, 100.0, 28.0));
            text(b, rect(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            clip(b, rect(0.0, 40.0, 100.0, 30.0));
            draw(b, rect(0.0, 40.0, 100.0, 28.0));
            text(b, rect(4.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.groups.len() >= 2, "distinct scissors → distinct groups");
    assert_eq!(
        buf.text_batches.len(),
        1,
        "non-overlapping rows must share one text batch",
    );
    assert_eq!(buf.text_batches[0].texts.len, 2);
}

/// Pin: a text run whose ancestor clip cuts its full extent must end
/// up in a batch whose GPU scissor equals exactly its clipped bounds —
/// the text shader has no per-instance clip, so a merged scissor would
/// let glyphs paint past the intended clip. Wider neighbour text on
/// the other side of the strict clip forces a split.
#[test]
fn compose_clipped_text_overflow_does_not_widen_batch_scissor() {
    let buf = run(
        |b, _arena| {
            // Wide outer text — unclipped, full bbox.
            text(b, rect(0.0, 0.0, 200.0, 20.0));
            // Narrow clip (20px wide) wrapping a wide text run (100px).
            // The run's intended visible region is 20px, but its
            // measured rect is 100px — the clip is the only thing
            // keeping the glyphs inside.
            clip(b, rect(40.0, 40.0, 20.0, 20.0));
            text(b, rect(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    // Two batches: one for the unclipped run, one for the strict one.
    // The strict batch's scissor must be the 20×20 clip rect — not
    // the union with the wide neighbour.
    assert_eq!(
        buf.text_batches.len(),
        2,
        "strict (clipped-narrower) text must not coalesce with wider neighbours",
    );
    let strict = buf
        .text_batches
        .iter()
        .find(|tb| tb.scissor.size.x == 20)
        .expect("expected a batch with 20px-wide scissor");
    assert_eq!(strict.scissor.size.x, 20);
    assert_eq!(strict.scissor.size.y, 20);
}

/// Pin: two strict runs whose clips happen to be IDENTICAL rects can
/// coalesce into one batch — the GPU scissor matches both. Important
/// for repeated strict clips (e.g. a column of clipped numeric inputs
/// all the same width).
#[test]
fn compose_strict_text_with_matching_clip_coalesces() {
    let clip_rect = rect(40.0, 40.0, 20.0, 20.0);
    let buf = run(
        |b, _arena| {
            clip(b, clip_rect);
            text(b, rect(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
            clip(b, clip_rect);
            text(b, rect(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(
        buf.text_batches.len(),
        1,
        "two strict runs with identical clip bounds should share a batch",
    );
}

/// Pin: a rounded-clip change splits the text batch even when text
/// across the change wouldn't otherwise overlap. Different rounded
/// clips → different stencil masks at render time; one merged prepare
/// would mis-clip text under one of them. Each batch also carries the
/// mask chain its runs were recorded under, value-matching its
/// `last_group`'s chain — the schedule needs it to stencil a batch
/// drained past damage-skipped groups against the right mask.
#[test]
fn compose_rounded_clip_change_splits_text_batch() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, rect(0.0, 0.0, 100.0, 30.0), Corners::all(4.0));
            text(b, rect(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            clip_rounded(b, rect(0.0, 40.0, 100.0, 30.0), Corners::all(8.0));
            text(b, rect(4.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.text_batches.len(), 2, "rounded change must split batch");
    for (i, tb) in buf.text_batches.iter().enumerate() {
        let batch_chain = &buf.rounded_clips[tb.rounded_clips.range()];
        assert_eq!(batch_chain.len(), 1, "batch {i} recorded under one mask");
        let group_chain =
            &buf.rounded_clips[buf.groups[tb.last_group as usize].rounded_clips.range()];
        assert_eq!(
            batch_chain, group_chain,
            "batch {i} chain matches its last_group's chain"
        );
    }
    // The two batches carry the two DIFFERENT masks (r4 vs r8).
    let r0 = buf.rounded_clips[buf.text_batches[0].rounded_clips.range()][0];
    let r1 = buf.rounded_clips[buf.text_batches[1].rounded_clips.range()][0];
    assert_eq!(r0.corners.as_array()[0], 4.0);
    assert_eq!(r1.corners.as_array()[0], 8.0);
}

/// Wiring for the paint-time spin (spinner): the composer must read
/// `DrawPolylinePayload::rotation` and rotate each point about
/// `bbox.center()` before the ancestor transform. A horizontal segment
/// through the box centre, spun 90°, comes out vertical and stays
/// centred on the pivot — catches a dropped rotation or a wrong pivot
/// that the analytic geometry test in `spinner` can't see.
#[test]
fn compose_spins_polyline_about_bbox_center() {
    // bbox 100×100 ⇒ centre (50, 50) is both the pivot and the symmetry
    // point of the segment, so a correct spin keeps the AABB centred.
    let aabb = |rotation: f32| -> (Vec2, Vec2) {
        let mut buffer = PaintCapture::default();
        let mut payloads = RecordPayloads::default();
        let p_start = payloads.polyline_points.len() as u32;
        payloads.polyline_points.push(Vec2::new(15.0, 50.0));
        payloads.polyline_points.push(Vec2::new(85.0, 50.0));
        let c_start = payloads.polyline_colors.len() as u32;
        payloads.polyline_colors.push(Color::WHITE.into());
        buffer.draw_polyline(DrawPolylinePayload {
            // Pivot is the 100x100 box centre, which `stroke_bounds`
            // derives from the owner rect on the production path.
            bounds: if rotation == 0.0 {
                StrokeBounds::Still(rect(0.0, 0.0, 100.0, 100.0))
            } else {
                StrokeBounds::Spun {
                    spin: Spin {
                        pivot: Vec2::splat(50.0),
                        angle: rotation,
                    },
                    radius: Vec2::splat(50.0).length(),
                }
            },
            origin: Vec2::ZERO,
            width: 2.0,
            points_start: p_start,
            points_len: 2,
            colors_start: c_start,
            colors_len: 1,
            color_mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        });
        let mut composer = composer();
        let mut out = render_buffer();
        composer
            .begin(
                params(1.0, UVec2::new(200, 200)),
                Duration::ZERO,
                &payloads,
                &mut out,
            )
            .replay_from(&buffer);
        // GPU path: the polyline emits one segment instance whose
        // p0/p3 lanes carry the transformed (spun) endpoints.
        assert_eq!(out.curves.len(), 1, "one segment instance");
        let ci = &out.curves[0];
        (ci.p0.min(ci.p3), ci.p0.max(ci.p3))
    };
    let (lo0, hi0) = aabb(0.0);
    let (lor, hir) = aabb(FRAC_PI_2);
    // Unrotated: a wide AABB (horizontal stroke).
    assert!(
        hi0.x - lo0.x > hi0.y - lo0.y,
        "unrotated stroke should be wide: {lo0:?}..{hi0:?}",
    );
    // Spun 90°: a tall AABB (vertical stroke) — proves rotation applied.
    assert!(
        hir.y - lor.y > hir.x - lor.x,
        "90° spin should be tall: {lor:?}..{hir:?}",
    );
    // Both stay centred on the pivot — proves the pivot is bbox.center().
    let c0 = (lo0 + hi0) * 0.5;
    let cr = (lor + hir) * 0.5;
    assert!(
        (c0 - Vec2::splat(50.0)).length() < 2.0,
        "unrotated centre {c0:?}"
    );
    assert!(
        (cr - Vec2::splat(50.0)).length() < 2.0,
        "spun centre {cr:?}"
    );
}

/// Pin: a higher-kind draw that gets *culled* (fully outside the active
/// clip) does NOT split the text batch — the batch only closes once the
/// draw will actually emit. Counterpart to
/// `compose_mesh_between_texts_splits_text_batch`.
#[test]
fn compose_culled_mesh_between_texts_keeps_one_batch() {
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            mesh(b, rect(200.0, 200.0, 30.0, 30.0)); // outside the clip → culled
            text(b, rect(0.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.meshes.len(), 0, "the mesh must be culled");
    assert_eq!(
        buf.text_batches.len(),
        1,
        "a culled mesh must not split the text batch",
    );
}

/// Pin: a quad that overlaps prior batch text closes the batch — the
/// merged batch would otherwise paint that text over the occluding
/// quad. Two groups, two text batches; quad in the middle.
#[test]
fn compose_quad_overlap_with_prior_batch_text_splits_batch() {
    let buf = run(
        |b, _arena| {
            text(b, rect(0.0, 0.0, 100.0, 30.0)); // text A
            // Push a clip to force a fresh group; quad inside overlaps text A.
            clip(b, rect(0.0, 0.0, 200.0, 200.0));
            draw(b, rect(10.0, 10.0, 50.0, 20.0)); // overlaps A → must close batch
            b.pop_clip();
            text(b, rect(0.0, 40.0, 100.0, 30.0)); // text B
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        buf.text_batches.len(),
        2,
        "quad overlapping prior batch text must split the batch",
    );
}

#[test]
fn tight_curve_bound_avoids_false_group_split() {
    #[derive(Debug)]
    struct Case {
        image_x: f32,
        expected_groups: usize,
    }

    // Curve centerline ends at x=20. Width 2 + 0.5 AA gives a
    // physical bound ending at ceil(21.5)=22. Touching x=22 is
    // disjoint; moving the image one pixel left creates real overlap.
    let cases = [
        Case {
            image_x: 22.0,
            expected_groups: 1,
        },
        Case {
            image_x: 21.0,
            expected_groups: 2,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _| {
                curve(b, rect(0.0, 0.0, 20.0, 20.0));
                image(b, rect(case.image_x, 0.0, 10.0, 10.0));
            },
            &params(1.0, UVec2::new(100, 100)),
        );
        assert_eq!(buf.groups.len(), case.expected_groups, "{case:?}");
    }
}

/// Counter-pin: record [mesh, curve] — the replay order mesh→curve
/// already matches record order, so both stay in one group.
#[test]
fn compose_mesh_then_overlapping_curve_keeps_one_group() {
    let buf = run(
        |b, _| {
            mesh(b, rect(10.0, 10.0, 30.0, 30.0));
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "record order matches replay order");
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
}

/// Mesh→image replays in record order (mesh drains before image in
/// `emit_group_body`) → one group; image→mesh inverts it (the later-
/// recorded mesh would drain first) → flush into two groups.
#[test]
fn compose_mesh_image_record_order_gates_group_split() {
    let buf = run(
        |b, _| {
            mesh(b, rect(10.0, 10.0, 30.0, 30.0));
            image(b, rect(20.0, 20.0, 30.0, 30.0)); // overlaps the mesh
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "mesh then image: replay == record");
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);

    let buf = run(
        |b, _| {
            image(b, rect(20.0, 20.0, 30.0, 30.0));
            mesh(b, rect(10.0, 10.0, 30.0, 30.0)); // overlaps the image
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.groups.len(),
        2,
        "image then mesh: replay inverts record",
    );
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 1);
}

/// Non-overlapping mixed kinds never conflict — record order between
/// disjoint draws is invisible, so they share one group (one draw call
/// per kind). Gaps exceed every bbox inflation: the curve at
/// (0,0,20,20) tracks (0,0)..(22,22) after its width/2 + 0.5 fringe,
/// the mesh at (40,40,20,20) tracks (39,39)..(61,61) after its 0.5
/// fringe, the image at (80,80,20,20) is exact.
#[test]
fn compose_disjoint_mixed_kinds_share_one_group() {
    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 0.0, 20.0, 20.0));
            mesh(b, rect(40.0, 40.0, 20.0, 20.0));
            image(b, rect(80.0, 80.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "disjoint kinds must not split");
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
}

//
// Pruning drops a quad iff a later quad in the same group fully
// covers its painted extent (`q.rect.inflated(stroke/2)`) under
// `Rect::contains_rect`.

/// Regression: a quad overlapping text that lives in an *already-closed*
/// batch within the same group must still flush so the text paints under
/// it. Reproduces the node-label-over-inspector-panel bug: a node's text
/// gets closed into its own batch (here, by an unrelated polyline that
/// doesn't overlap), then the panel quad — recorded later, overlapping —
/// must not let that closed batch's text paint on top.
#[test]
fn quad_flushes_text_in_already_closed_batch_same_group() {
    let buf = run(
        |b, payloads| {
            // First node label.
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            // A polyline far from everything closes the text batch
            // (curve-tier) without flushing the group, and doesn't
            // overlap the quad below (so it can't be what forces the
            // flush).
            polyline_cmd(
                b,
                payloads,
                &[Vec2::new(0.0, 400.0), Vec2::new(50.0, 400.0)],
                &[Color::WHITE],
                ColorMode::Single,
                1.0,
                LineCap::Butt,
                LineJoin::Miter,
            );
            // Panel chrome quad, overlapping the (now closed-batch) label.
            draw(b, rect(0.0, 0.0, 100.0, 60.0));
            // Repeat after the first closed batch has been indexed and
            // flushed. The new pending tail must be discovered independently.
            text(b, rect(0.0, 100.0, 100.0, 20.0));
            polyline_cmd(
                b,
                payloads,
                &[Vec2::new(0.0, 500.0), Vec2::new(50.0, 500.0)],
                &[Color::WHITE],
                ColorMode::Single,
                1.0,
                LineCap::Butt,
                LineJoin::Miter,
            );
            draw(b, rect(0.0, 100.0, 100.0, 60.0));
        },
        &params(1.0, UVec2::new(600, 600)),
    );
    assert_eq!(buf.text_batches.len(), 2);
    assert_eq!(buf.text_batches[0].scissor, URect::new(0, 0, 100, 20));
    assert_eq!(buf.text_batches[1].scissor, URect::new(0, 100, 100, 20));
    for (batch, quad_y) in buf.text_batches.iter().zip([0.0, 100.0]) {
        let quad_group = buf
            .groups
            .iter()
            .enumerate()
            .find(|(_, group)| {
                group
                    .quads
                    .range()
                    .any(|qi| buf.quads[qi].rect.min.y == quad_y)
            })
            .map(|(i, _)| i as u32)
            .expect("panel quad group");
        assert!(
            batch.last_group < quad_group,
            "closed-batch text (last_group={}) must paint before the overlapping quad \
             at y={quad_y} (group={quad_group})",
            batch.last_group,
        );
    }
}

/// Fragment fast-path flag: solid + sharp + stroke-less + pixel-aligned
/// quads carry `FillKind::FAST_BIT`; any disqualifier (fractional rect,
/// corners, stroke, gradient) leaves the kind plain. Alignment is
/// checked on the *physical* rect, so a fractional logical rect at a
/// DPR that lands it on integers still qualifies — and translucency
/// does NOT disqualify (the skip is coverage-based, not opacity-based).
#[test]
fn quad_fast_path_flag_cases() {
    use crate::primitives::brush::gradient::FillAxis;
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::fill_wire::{FillKind, LutRow};

    let solid = |c: Color| BrushSource::Solid(c.into());
    let opaque = Color::rgb(0.5, 0.5, 0.5);

    // (case, rect, corners, stroke, brush, dpr, expect_fast)
    let gradient = BrushSource::Gradient(ResolvedGradient {
        axis: FillAxis::ZERO,
        row: LutRow::FALLBACK,
        kind: FillKind::linear(Spread::Pad),
    });
    let cases: &[(&str, Rect, Corners, Stroke, BrushSource, f32, bool)] = &[
        (
            "aligned sharp strokeless solid",
            rect(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            solid(opaque),
            1.0,
            true,
        ),
        (
            "translucent still qualifies",
            rect(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            solid(Color::rgba(0.5, 0.5, 0.5, 0.5)),
            1.0,
            true,
        ),
        (
            "fractional logical rect aligned at DPR 2",
            rect(10.5, 10.5, 20.0, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            solid(opaque),
            2.0,
            true,
        ),
        (
            "fractional rect disqualifies",
            rect(10.25, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "fractional size disqualifies",
            rect(10.0, 10.0, 20.5, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "corners disqualify",
            rect(10.0, 10.0, 20.0, 20.0),
            Corners::all(4.0),
            Stroke::ZERO,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "stroke disqualifies",
            rect(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::solid(Color::WHITE, 1.0),
            solid(opaque),
            1.0,
            false,
        ),
        (
            "gradient disqualifies",
            rect(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::ZERO,
            gradient,
            1.0,
            false,
        ),
    ];

    for (name, r, corners, stroke, brush, dpr, expect_fast) in cases {
        let buf = run(
            |b, _arena| {
                b.draw_quad(DrawQuadPayload::rect(
                    *r,
                    *corners,
                    *brush,
                    (*stroke).into(),
                ))
            },
            &params(*dpr, UVec2::new(400, 400)),
        );
        assert_eq!(buf.quads.len(), 1, "{name}: quad emitted");
        let got = buf.quads[0].fill_kind;
        let plain = match brush {
            BrushSource::Solid(_) => FillKind::SOLID,
            BrushSource::Gradient(g) => g.kind,
        };
        let want = if *expect_fast {
            plain.with_fast()
        } else {
            plain
        };
        assert_eq!(got, want, "{name}: fill_kind");
    }
}

/// What a labelled toolbar actually costs in batches — the measurement behind
/// keeping the icon atlas separate from the glyph atlas rather than folding
/// the two together.
///
/// Eight buttons, each an icon beside its label, laid out left to right with
/// no overlap. Icons are a higher kind than text, so every icon closes the
/// open text batch: the icons coalesce into one batch (they accumulate in the
/// group until it flushes) while the text splits into one batch per run.
///
/// The number this pins is that the split is **text's**, not the icon
/// atlas's — merging the two atlases would remove the tier boundary and so the
/// splits, but the same 8-way split already happens today for eight images or
/// eight meshes interleaved with labels. That is what makes this a general
/// tier-ordering cost rather than something icons introduced.
#[test]
fn labelled_toolbar_costs_one_icon_batch_and_a_text_batch_per_label() {
    use crate::renderer::frontend::composer::tests::support::{icon, icon_ref};

    const BUTTONS: usize = 8;
    let out = run(
        |buf, _| {
            for i in 0..BUTTONS {
                let x = i as f32 * 100.0;
                icon(buf, rect(x, 0.0, 16.0, 16.0), icon_ref(i as u16));
                text(buf, rect(x + 20.0, 0.0, 60.0, 16.0));
            }
        },
        &params(1.0, UVec2::new(1024, 64)),
    );

    assert_eq!(out.icons.len(), BUTTONS);
    assert_eq!(
        out.batches(PaintTier::Icon).len(),
        1,
        "every icon shares one atlas, so they are one draw however many buttons",
    );
    assert_eq!(
        out.text_batches.len(),
        BUTTONS,
        "each icon closes the open text batch, so labels do not coalesce",
    );
    assert_eq!(out.groups.len(), 1, "disjoint draws need no group flush");
}

/// The control for the test above: the same eight labels with an *image*
/// between them instead of an icon split text identically. The cost belongs to
/// the tier boundary, not to icons having their own atlas.
#[test]
fn images_between_labels_split_text_the_same_way() {
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::frontend::composer::tests::support::gpu_view_payload;

    const BUTTONS: usize = 8;
    let out = run(
        |buf, _| {
            for i in 0..BUTTONS {
                let x = i as f32 * 100.0;
                buf.draw_image(
                    gpu_view_payload(rect(x, 0.0, 16.0, 16.0), TextureId(1)),
                    None,
                );
                text(buf, rect(x + 20.0, 0.0, 60.0, 16.0));
            }
        },
        &params(1.0, UVec2::new(1024, 64)),
    );
    assert_eq!(out.text_batches.len(), BUTTONS);
}
