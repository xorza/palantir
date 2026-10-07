//! What a clip keeps, what it culls, and what a rounded one costs.

use crate::common::span::Span;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::tests::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::tests::support::{
    clip, clip_rounded, curve, draw, draw_marked, image, mesh, params, push_distinct_rounded_clips,
    run, survivor_calls, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
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
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
}

/// Closing is the session's destructor, so dropping it still emits the trailing group and text batch; nothing else surfaces the omission.
#[test]
fn dropping_a_session_emits_the_trailing_group_and_batch() {
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    {
        let mut session = rig
            .composer
            .begin(rig.display, Duration::ZERO, &rig.store, &mut rig.out);
        let mut recorded = PaintCapture::default();
        draw(&mut recorded, Rect::new(0.0, 0.0, 10.0, 10.0));
        text(&mut recorded, Rect::new(0.0, 20.0, 10.0, 10.0));
        recorded.replay(&mut session);
        // Rows are buffered; only the group and batch that schedule them are pending.
        assert_eq!(session.out.quads.len(), 1);
        assert_eq!(session.out.texts.len(), 1);
        assert!(session.out.groups.is_empty());
        assert!(session.out.text_batches.is_empty());
    }
    let out = &rig.out;
    assert_eq!(out.quads.len(), 1);
    assert_eq!(out.texts.len(), 1);
    assert_eq!(out.groups.len(), 1, "trailing group emitted on drop");
    assert_eq!(out.groups[0].quads, Span::new(0, 1));
    assert_eq!(out.text_batches.len(), 1, "trailing batch closed on drop");
    assert_eq!(out.text_batches[0].texts, Span::new(0, 1));
    assert_eq!(out.text_batches[0].last_group, 0);
}

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            clip(b, Rect::new(50.0, 50.0, 100.0, 100.0));
            draw(b, Rect::new(60.0, 60.0, 20.0, 20.0));
            draw(b, Rect::new(90.0, 90.0, 20.0, 20.0));
            b.pop_clip();
            draw(b, Rect::new(0.0, 0.0, 5.0, 5.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 4);
    assert_eq!(buf.groups.len(), 3);

    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));

    let s = buf.groups[1]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.min.x, s.min.y, s.size.x, s.size.y), (50, 50, 100, 100));
    assert_eq!(buf.groups[1].quads, Span::new(1, 2));

    assert!(buf.groups[2].scissor.is_none());
    assert_eq!(buf.groups[2].quads, Span::new(3, 1));
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
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("nested clip group must have a scissor");
    assert_eq!((s.min.x, s.min.y, s.size.x, s.size.y), (50, 50, 50, 50));
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
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.groups.len(), 1);
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
        buf.groups.len(),
        1,
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
    assert!(buf.groups.is_empty());
}

/// Rounded clip rides on the emitted `DrawGroup` as a one-entry mask chain, scaled by DPR; a nested `Rect` clip inherits the chain, else inner draws land at `stencil_ref=0` over `stencil=1` and vanish.
#[test]
fn push_clip_rounded_lands_radius_on_group_and_inherits_through_rect() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 20.0, 100.0, 80.0), Corners::all(8.0));
            // Tier 1: direct draw under the rounded clip.
            draw(b, Rect::new(20.0, 30.0, 40.0, 40.0));
            // Tier 2: a nested plain rect clip must still inherit the rounded info.
            clip(b, Rect::new(30.0, 40.0, 40.0, 30.0));
            draw(b, Rect::new(35.0, 45.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert!(!buf.rounded_clips.is_empty());
    assert_eq!(
        buf.groups.len(),
        2,
        "two groups: outer rounded scissor, inner rect scissor"
    );

    let outer = &buf.groups[0];
    let inner = &buf.groups[1];

    let outer_chain = &buf.rounded_clips[outer.rounded_clips.range()];
    assert_eq!(outer_chain.len(), 1, "single rounded clip → depth-1 chain");
    let outer_r = outer_chain[0];
    // DPR=2 → radius doubles 8→16, rect (10,20,100,80) → (20,40,200,160).
    assert_eq!(outer_r.corners.as_array()[0], 16.0);
    assert_eq!(outer_r.mask_rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(outer_r.mask_rect.size, Size::new(200.0, 160.0));
    assert_eq!(outer.scissor, Some(URect::new(20, 40, 200, 160)));

    // Inheritance: the inner Rect clip carries the outer chain; scissor narrows independently.
    assert_eq!(
        inner.rounded_clips, outer.rounded_clips,
        "inner group inherits parent's mask chain verbatim"
    );
    // DPR=2: rect (30,40,40,30) → (60,80,80,60), clamped to outer.
    assert_eq!(inner.scissor, Some(URect::new(60, 80, 80, 60)));
}

/// Nested rounded clips stack: the child chain lists both masks outer→inner (a single mask would paint the child square over the ancestor's cutouts); a nested rect clip inherits the depth-2 chain. DPR 1: outer (10,10,200,200) r8, inner (20,20,100,100) r4.
#[test]
fn push_clip_rounded_nested_builds_outer_inner_chain() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(10.0, 10.0, 200.0, 200.0), Corners::all(8.0));
            draw(b, Rect::new(20.0, 20.0, 40.0, 40.0));
            clip_rounded(b, Rect::new(20.0, 20.0, 100.0, 100.0), Corners::all(4.0));
            draw(b, Rect::new(30.0, 30.0, 20.0, 20.0));
            clip(b, Rect::new(30.0, 30.0, 50.0, 50.0));
            draw(b, Rect::new(35.0, 35.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        buf.groups.len(),
        3,
        "outer rounded, nested rounded, nested rect"
    );
    let chain = |g: usize| &buf.rounded_clips[buf.groups[g].rounded_clips.range()];

    let outer = chain(0);
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].mask_rect, Rect::new(10.0, 10.0, 200.0, 200.0));
    assert_eq!(outer[0].corners.as_array()[0], 8.0);

    let nested = chain(1);
    assert_eq!(nested.len(), 2, "nested rounded stacks on the ancestor");
    assert_eq!(
        nested[0], outer[0],
        "chain lists the ancestor first (outer→inner)"
    );
    assert_eq!(nested[1].mask_rect, Rect::new(20.0, 20.0, 100.0, 100.0));
    assert_eq!(nested[1].corners.as_array()[0], 4.0);

    // Rect clip under both: inherits the depth-2 chain verbatim.
    assert_eq!(
        buf.groups[2].rounded_clips, buf.groups[1].rounded_clips,
        "rect inside nested rounded inherits the full chain"
    );
    assert_eq!(buf.groups[2].scissor, Some(URect::new(30, 30, 50, 50)));
}

#[test]
fn rounded_clip_chain_accepts_stencil_depth_255() {
    let buf = run(
        |buffer, _payloads| {
            push_distinct_rounded_clips(buffer, 255);
            draw(buffer, Rect::new(100.0, 100.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );

    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].rounded_clips.len, 255);
}

#[test]
#[should_panic(expected = "rounded clip chain depth 256 exceeds stencil capacity 255")]
fn rounded_clip_chain_rejects_stencil_depth_256() {
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
            draw(b, Rect::new(20.0, 20.0, 20.0, 20.0));
            clip_rounded(b, Rect::new(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, Rect::new(50.0, 50.0, 20.0, 20.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 1, "identical rounded re-push is a no-op");
    assert_eq!(
        buf.rounded_clips[buf.groups[0].rounded_clips.range()].len(),
        1,
        "no extra chain level for the redundant mask"
    );
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
    let chain = &buf.rounded_clips[buf.groups[0].rounded_clips.range()];
    let r = chain[0];
    // Mask rect keeps the off-screen origin (-50,-20) and full size (200,100).
    assert_eq!(r.mask_rect.min, Vec2::new(-50.0, -20.0));
    assert_eq!(r.mask_rect.size, Size::new(200.0, 100.0));
    // Scissor clamps to the viewport.
    assert_eq!(buf.groups[0].scissor, Some(URect::new(0, 0, 120, 60)));
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
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.rounded_clips.is_empty());
    assert_eq!(buf.groups[0].rounded_clips.len, 0);
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
    assert!(buf.groups.is_empty());
    assert!(buf.batches(PaintTier::Mesh).is_empty());
    assert!(buf.batches(PaintTier::Image).is_empty());
    assert!(buf.batches(PaintTier::Curve).is_empty());
}
