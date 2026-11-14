//! What a clip keeps, what it culls, and what a rounded one costs.

use crate::common::span::Span;
use crate::display::frame_stamp::FrameStamp;
use crate::internals::compose_rig::ComposeRig;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::tests::support::{
    clip, clip_rounded, curve, draw, draw_marked, image, mesh, params, push_distinct_rounded_clips,
    quad_runs, run, survivor_calls, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::draw_run::{DrawRun, Lane};
use glam::{UVec2, Vec2};
use std::time::Duration;

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            draw(b, Rect::new(20.0, 0.0, 10.0, 10.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(quad_runs(&buf), [(Span::new(0, 2), None)]);
}

/// Finishing a session lists the trailing group's runs, its quads then its text batch, which its
/// rows wait on until then.
#[test]
fn finishing_a_session_emits_the_trailing_group_and_batch() {
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    {
        let mut session = rig.composer.begin(
            FrameStamp::new(rig.display, Duration::ZERO),
            &rig.store,
            &mut rig.out,
        );
        let mut recorded = PaintCapture::default();
        draw(&mut recorded, Rect::new(0.0, 0.0, 10.0, 10.0));
        text(&mut recorded, Rect::new(0.0, 20.0, 10.0, 10.0));
        recorded.replay(&mut session);
        // Rows are buffered; only the runs that schedule them are pending.
        assert_eq!(session.out.quads.len(), 1);
        assert_eq!(session.out.texts.len(), 1);
        assert!(session.out.runs.is_empty());
        session.finish();
    }
    let out = &rig.out;
    assert_eq!(out.quads.len(), 1);
    assert_eq!(out.texts.len(), 1);
    assert_eq!(
        out.runs,
        [
            DrawRun {
                lane: Lane::Quad,
                rows: Span::new(0, 1),
                scissor: None,
                bounds: URect::new(0, 0, 10, 10),
                batch: 0,
            },
            DrawRun {
                lane: Lane::Text,
                rows: Span::new(0, 1),
                scissor: None,
                // The run's bounds padded by the scale step's slack and covered.
                bounds: URect::new(0, 19, 11, 12),
                batch: 0,
            },
        ],
        "the trailing group and batch list on finish"
    );
}

/// A clip closes no group, and costs a scissor only where it cuts a quad: under (50, 50, 100 ×
/// 100), the quad at (140, 140) crosses its corner and draws under it, and so does the one inside
/// it, as one scissor draws them anyway; the quads outside the clip, before and after it, join
/// one run under none, the last moving back past the clipped ones it does not overlap.
#[test]
fn a_clip_scissors_the_quads_it_cuts() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            draw(b, Rect::new(60.0, 60.0, 20.0, 20.0));
            draw(b, Rect::new(140.0, 140.0, 20.0, 20.0));
            b.pop_clip();
            draw(b, Rect::new(0.0, 0.0, 5.0, 5.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let rects: Vec<Rect> = buf.quads.instance().iter().map(|q| q.header.rect).collect();
    assert_eq!(
        rects,
        [
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(0.0, 0.0, 5.0, 5.0),
            Rect::new(60.0, 60.0, 20.0, 20.0),
            Rect::new(140.0, 140.0, 20.0, 20.0),
        ]
    );
    assert_eq!(
        quad_runs(&buf),
        [
            (Span::new(0, 2), None),
            (Span::new(2, 2), Some(URect::new(50, 50, 100, 100))),
        ]
    );
}

/// A tier draw joins its open run while that run's scissor cuts it as its own clip does: under
/// (50, 50, 100 × 100) the image across the clip's corner opens a run under it, the one inside it
/// joins, as the clip holds it, and the one past the clip, which the clip would cut away, opens a
/// run under none; one across the corner again, under the clip pushed again, opens a third. Each
/// run's bounds are the images' union in the group, (60, 10)..(220, 150), cut to its scissor:
/// wider than its own draws, which costs a damage walk a skip, never a pixel. The group lists its
/// two closed runs, then its open one.
#[test]
fn a_tier_draw_joins_its_open_run_while_its_scissor_cuts_it_as_its_clip_does() {
    let clip_rect = URect::new(50, 50, 100, 100);
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            image(b, Rect::new(140.0, 140.0, 20.0, 20.0));
            image(b, Rect::new(60.0, 60.0, 20.0, 20.0));
            b.pop_clip();
            image(b, Rect::new(200.0, 10.0, 20.0, 20.0));
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            image(b, Rect::new(140.0, 140.0, 20.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let runs: Vec<(Span, Option<URect>, URect)> = buf
        .runs
        .iter()
        .map(|run| (run.rows, run.scissor, run.bounds))
        .collect();
    assert_eq!(
        runs,
        [
            (
                Span::new(0, 2),
                Some(clip_rect),
                URect::new(60, 50, 90, 100)
            ),
            (Span::new(2, 1), None, URect::new(60, 10, 160, 140)),
            (
                Span::new(3, 1),
                Some(clip_rect),
                URect::new(60, 50, 90, 100)
            ),
        ]
    );
}

/// A quad its clip does not cut draws under no scissor, where no quad of its group needs the
/// clip: the clipped quad joins the run of the quad before the clip.
#[test]
fn a_clip_that_cuts_nothing_costs_no_run() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            draw(b, Rect::new(60.0, 60.0, 20.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(quad_runs(&buf), [(Span::new(0, 2), None)]);
}

#[test]
fn compose_intersects_nested_clips() {
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            draw(b, Rect::new(60.0, 60.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 1), Some(URect::new(50, 50, 50, 50)))]
    );
}

/// Every draw kind culls against the active clip alike: wholly outside is dropped, any overlap kept. Under a 100 px clip each kind draws one rect inside, one at (200, 200), and one straddling the corner, so two rows survive.
#[test]
fn cull_drops_only_draws_wholly_outside_the_active_clip() {
    #[derive(Debug)]
    struct Kind {
        name: &'static str,
        draw: fn(&mut PaintCapture, Rect),
        rows: fn(&RenderBuffer) -> usize,
    }
    let kinds = [
        Kind {
            name: "rect",
            draw,
            rows: |buf| buf.quads.len(),
        },
        Kind {
            name: "text",
            draw: text,
            rows: |buf| buf.texts.len(),
        },
        Kind {
            name: "mesh",
            draw: mesh,
            rows: |buf| buf.meshes.len(),
        },
    ];
    for kind in kinds {
        let buf = run(
            |b, _arena| {
                clip(b, Rect::new(0.0, 0.0, 100.0, 100.0));
                (kind.draw)(b, Rect::new(10.0, 10.0, 30.0, 30.0));
                (kind.draw)(b, Rect::new(200.0, 200.0, 30.0, 30.0));
                (kind.draw)(b, Rect::new(80.0, 80.0, 50.0, 50.0));
                b.pop_clip();
            },
            &params(1.0, UVec2::new(400, 400)),
        );
        assert_eq!((kind.rows)(&buf), 2, "{}", kind.name);
    }
}

#[test]
fn cull_without_active_clip_keeps_nonzero_viewport_bounds() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(-10.0, -10.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(quad_runs(&buf), [(Span::new(0, 1), None)]);
}

#[test]
fn cull_handles_culled_text_then_quad_split() {
    // A culled text run must not flag `last_was_text`, or the next quad forces a spurious group flush: [text-out, rect-in, rect-in] share one group.
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            text(b, Rect::new(300.0, 300.0, 50.0, 20.0)); // culled
            draw_marked(b, Rect::new(10.0, 10.0, 30.0, 30.0)); // call 2
            draw_marked(b, Rect::new(50.0, 50.0, 30.0, 30.0)); // call 3
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 0);
    assert_eq!(survivor_calls(&buf), [2, 3]);
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 2), Some(URect::new(0, 0, 100, 100)))],
        "culled text must not flag last_was_text and split the group"
    );
}

#[test]
fn compose_skips_groups_with_no_quads() {
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 50.0, 50.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.runs.is_empty());
}

/// A rounded clip rides on each draw under it that reaches its edge as a one-entry chain, scaled
/// by DPR; a nested `Rect` clip inherits the chain, which still cuts what it holds. Both draws
/// cross the rounded clip's left edge at x = 10.
#[test]
fn push_clip_rounded_lands_radius_on_group_and_inherits_through_rect() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 20.0, 100.0, 80.0), Corners::all(8.0));
            // Tier 1: direct draw under the rounded clip.
            draw(b, Rect::new(5.0, 30.0, 40.0, 40.0));
            // Tier 2: a nested plain rect clip must still inherit the rounded info.
            clip(b, Rect::new(5.0, 25.0, 40.0, 30.0));
            draw(b, Rect::new(5.0, 30.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert!(!buf.rounded_clips.is_empty());
    let [(_, outer), (_, inner)] = quad_runs(&buf)[..] else {
        panic!("two groups: outer rounded scissor, inner rect scissor");
    };

    let chain = |q: usize| &buf.rounded_clips[buf.quads.instance()[q].header.clip.span().range()];
    let outer_chain = chain(0);
    assert_eq!(outer_chain.len(), 1, "single rounded clip → depth-1 chain");
    let outer_r = outer_chain[0];
    // DPR=2 → radius doubles 8→16, rect (10,20,100,80) → (20,40,200,160).
    assert_eq!(outer_r.corners.top_left(), 16.0);
    assert_eq!(outer_r.mask_rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(outer_r.mask_rect.size, Size::new(200.0, 160.0));
    assert_eq!(outer, Some(URect::new(20, 40, 200, 160)));

    // Inheritance: the inner Rect clip carries the outer chain; scissor narrows independently.
    assert_eq!(
        buf.quads.instance()[1].header.clip,
        buf.quads.instance()[0].header.clip,
        "inner draw inherits parent's chain verbatim"
    );
    // DPR=2: rect (5,25,40,30) → (10,50,80,60), clamped to outer.
    assert_eq!(inner, Some(URect::new(20, 50, 70, 60)));
}

/// Nested rounded clips stack where neither holds the next, outer→inner; an ancestor that holds the
/// inner clip's reach leaves the chain. DPR 1: outer (10,10,200,200) r8 and inner
/// (20,20,100,100) r4: the inner's rect grown by 0.5 reaches (19.5, 19.5), whose distance to the
/// outer's arc about (18, 18) is `√(1.5² + 1.5²) − 8 ≈ −5.88`, past the 0.52 the outer needs to
/// cover it whole, so the inner's chain is the inner alone; a nested rect clip inherits it. A third
/// clip (5, 20, 100, 100) r4 crosses the outer's left edge, so it keeps the outer: depth 2.
#[test]
fn push_clip_rounded_nested_builds_outer_inner_chain() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 10.0, 200.0, 200.0), Corners::all(8.0));
            draw(b, Rect::new(5.0, 20.0, 40.0, 40.0));
            clip_rounded(b, Rect::new(20.0, 20.0, 100.0, 100.0), Corners::all(4.0));
            draw(b, Rect::new(15.0, 30.0, 20.0, 20.0));
            clip(b, Rect::new(20.0, 30.0, 50.0, 50.0));
            draw(b, Rect::new(15.0, 35.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
            clip_rounded(b, Rect::new(5.0, 20.0, 100.0, 100.0), Corners::all(4.0));
            draw(b, Rect::new(0.0, 30.0, 20.0, 20.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let scissors: Vec<Option<URect>> = quad_runs(&buf).iter().map(|run| run.1).collect();
    assert_eq!(
        scissors.len(),
        4,
        "outer rounded, nested rounded, nested rect, crossing rounded"
    );
    let chain = |q: usize| &buf.rounded_clips[buf.quads.instance()[q].header.clip.span().range()];

    let outer = chain(0);
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].mask_rect, Rect::new(10.0, 10.0, 200.0, 200.0));
    assert_eq!(outer[0].corners.top_left(), 8.0);

    let nested = chain(1);
    assert_eq!(
        nested.len(),
        1,
        "the outer holds the inner and leaves its chain"
    );
    assert_eq!(nested[0].mask_rect, Rect::new(20.0, 20.0, 100.0, 100.0));
    assert_eq!(nested[0].corners.top_left(), 4.0);

    // Rect clip under both: inherits the inner's chain verbatim.
    assert_eq!(
        buf.quads.instance()[2].header.clip,
        buf.quads.instance()[1].header.clip,
        "rect inside nested rounded inherits the full chain"
    );
    assert_eq!(scissors[2], Some(URect::new(20, 30, 50, 50)));

    let crossing = chain(3);
    assert_eq!(crossing.len(), 2, "a clip past the outer's edge keeps it");
    assert_eq!(crossing[0], outer[0], "the chain lists the ancestor first");
    assert_eq!(crossing[1].mask_rect, Rect::new(5.0, 20.0, 100.0, 100.0));
}

/// A draw whose pixel centers every clip of its chain covers whole takes no clip; one that reaches
/// a clip's edge band takes the chain. Under (10, 10, 100, 80) r8: (40, 40, 10 × 10) lies far
/// inside; (5, 30, 20 × 10) crosses the left edge; (10, 10, 6 × 6) lies inside the rect but its
/// top-left pixel center (10.5, 10.5) sits `√(7.5² · 2) − 8 ≈ 2.6` past the arc about (18, 18).
#[test]
fn a_draw_inside_its_clips_takes_none() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 10.0, 100.0, 80.0), Corners::all(8.0));
            draw(b, Rect::new(40.0, 40.0, 10.0, 10.0));
            draw(b, Rect::new(5.0, 30.0, 20.0, 10.0));
            draw(b, Rect::new(10.0, 10.0, 6.0, 6.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 3);
    assert_eq!(
        buf.quads.instance()[0].header.clip,
        ClipChain::NONE,
        "far inside"
    );
    assert_eq!(
        buf.quads.instance()[1].header.clip.span().len,
        1,
        "across the edge"
    );
    assert_eq!(
        buf.quads.instance()[2].header.clip.span().len,
        1,
        "past the arc"
    );
    assert!(buf.clips_drawn, "a draw names a chain");
    let inside = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 10.0, 100.0, 80.0), Corners::all(8.0));
            draw(b, Rect::new(40.0, 40.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert!(
        !inside.clips_drawn,
        "a frame whose draws lie inside their clips names none"
    );
}

#[test]
fn rounded_clip_chain_accepts_depth_255() {
    let buf = run(
        |buffer, _payloads| {
            push_distinct_rounded_clips(buffer, 255);
            // Across every clip's left edge, so it takes the whole chain.
            draw(buffer, Rect::new(-5.0, 100.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );

    assert_eq!(quad_runs(&buf).len(), 1);
    assert_eq!(buf.quads.instance()[0].header.clip.span().len, 255);
}

#[test]
#[should_panic(expected = "rounded clip chain depth 256 exceeds the deepest chain, 255")]
fn rounded_clip_chain_rejects_depth_256() {
    let _ = run(
        |buffer, _payloads| push_distinct_rounded_clips(buffer, 256),
        &params(1.0, UVec2::new(400, 400)),
    );
}

/// Re-pushing the innermost rounded clip verbatim adds no chain depth and is a full no-op: no batch split, no group flush.
#[test]
fn push_clip_rounded_redundant_identical_push_adds_no_depth() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, Rect::new(5.0, 20.0, 20.0, 20.0));
            clip_rounded(b, Rect::new(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, Rect::new(5.0, 50.0, 20.0, 20.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 2), Some(URect::new(10, 10, 100, 100)))],
        "identical rounded re-push is a no-op"
    );
    assert_eq!(
        buf.quads.instance()[1].header.clip.span().len,
        1,
        "no extra chain level for the redundant mask"
    );
    assert_eq!(
        buf.quads.instance()[1].header.clip,
        buf.quads.instance()[0].header.clip
    );
    assert_eq!(buf.rounded_clips.len(), 1, "the pool holds the one clip");
}

/// Regression: when a rounded clip partly leaves the viewport the scissor clamps, but the mask SDF must keep the rect's true edges, or corners slide inward.
#[test]
fn push_clip_rounded_mask_rect_is_unclamped_to_viewport() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(-50.0, -20.0, 200.0, 100.0), Corners::all(8.0));
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(120, 60)),
    );
    // Every pixel on screen lies far inside the clip, so the draw takes no chain; the pool holds it.
    assert_eq!(buf.quads.instance()[0].header.clip, ClipChain::NONE);
    let r = buf.rounded_clips[0];
    // Mask rect keeps the off-screen origin (-50,-20) and full size (200,100).
    assert_eq!(r.mask_rect.min, Vec2::new(-50.0, -20.0));
    assert_eq!(r.mask_rect.size, Size::new(200.0, 100.0));
    // Scissor clamps to the viewport.
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 1), Some(URect::new(0, 0, 120, 60)))]
    );
}

#[test]
fn push_clip_rect_emits_no_rounded_data() {
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(10.0, 20.0, 100.0, 80.0));
            draw(b, Rect::new(20.0, 30.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 1), Some(URect::new(10, 20, 100, 80)))]
    );
    assert!(buf.rounded_clips.is_empty());
    assert_eq!(buf.quads.instance()[0].header.clip, ClipChain::NONE);
    assert!(!buf.clips_drawn, "a rect clip names no rounded clip");
}

/// Every draw that reaches a rounded clip's edge names the chain in force, a quad, a text run, a
/// mesh, an image and a curve alike, and one past the clip names none: each crosses the clip's
/// left edge at x = 0, so its first pixel center, 0.5, lies within the edge band, and takes a
/// depth-1 chain at the pool's start, `ClipChain::new(Span::new(0, 1))`. A draw that names one
/// marks the frame as one whose draws name rounded clips.
#[test]
fn every_draw_names_the_chain_in_force() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(0.0, 0.0, 200.0, 200.0), Corners::all(8.0));
            draw(b, Rect::new(-5.0, 10.0, 20.0, 20.0));
            text(b, Rect::new(-5.0, 40.0, 60.0, 20.0));
            mesh(b, Rect::new(-5.0, 70.0, 20.0, 20.0));
            image(b, Rect::new(-5.0, 100.0, 20.0, 20.0));
            curve(b, Rect::new(-5.0, 130.0, 20.0, 20.0));
            b.pop_clip();
            draw(b, Rect::new(250.0, 10.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let edge = ClipChain::new(Span::new(0, 1));
    assert_eq!(buf.quads.instance()[0].header.clip, edge);
    assert_eq!(buf.texts[0].clip, edge);
    assert_eq!(buf.meshes.instance()[0].clip, edge);
    assert_eq!(buf.images.instance()[0].header.clip, edge);
    assert!(buf.curves.iter().all(|c| c.clip == edge));
    assert_eq!(
        buf.quads.instance()[1].header.clip,
        ClipChain::NONE,
        "past the clip"
    );
    assert!(buf.clips_drawn);
}

#[test]
fn compose_culls_non_text_draws_outside_each_viewport_edge_without_clip() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(-40.0, 10.0, 10.0, 10.0));
            mesh(b, Rect::new(10.0, -40.0, 10.0, 10.0));
            image(b, Rect::new(240.0, 10.0, 10.0, 10.0));
            curve(b, Rect::new(10.0, 240.0, 10.0, 10.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.meshes.is_empty());
    assert!(buf.images.is_empty());
    assert!(buf.curves.is_empty());
    assert!(buf.runs.is_empty());
}
