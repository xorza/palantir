//! What a clip keeps, what it culls, and what a rounded one costs.

use crate::primitives::span::Span;
use crate::primitives::{corners::Corners, size::Size, urect::URect};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::composer::tests::support::{
    clip, clip_rounded, composer, curve, draw, image, mesh, params, push_distinct_rounded_clips,
    rect, render_buffer, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::record_payloads::RecordPayloads;
use glam::{UVec2, Vec2};
use std::time::Duration;

#[test]
#[should_panic(expected = "composer texture dimension limit must be positive")]
fn composer_rejects_zero_texture_limit() {
    let _ = Composer::new(0);
}

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            draw(b, rect(20.0, 0.0, 10.0, 10.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_none());
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
}

/// Closing is the session's destructor, so a caller that just drops it
/// still gets the trailing group and text batch. Nothing else surfaces
/// the omission: the quad and text rows land in the buffer either way,
/// and a backend given no group/batch covering them silently draws
/// neither.
#[test]
fn dropping_a_session_emits_the_trailing_group_and_batch() {
    let display = params(1.0, UVec2::new(200, 200));
    let payloads = RecordPayloads::default();
    let mut composer = composer();
    let mut out = render_buffer();
    {
        let mut session = composer.begin(display, Duration::ZERO, &payloads, &mut out);
        let mut recorded = PaintCapture::default();
        draw(&mut recorded, rect(0.0, 0.0, 10.0, 10.0));
        text(&mut recorded, rect(0.0, 20.0, 10.0, 10.0));
        recorded.replay(&mut session);
        // The rows themselves are already in the buffer; only the
        // group and batch that schedule them are still pending.
        assert_eq!(session.out.quads.len(), 1);
        assert_eq!(session.out.texts.len(), 1);
        assert!(session.out.groups.is_empty());
        assert!(session.out.text_batches.is_empty());
    }
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
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            clip(b, rect(50.0, 50.0, 100.0, 100.0));
            draw(b, rect(60.0, 60.0, 20.0, 20.0));
            draw(b, rect(90.0, 90.0, 20.0, 20.0));
            b.pop_clip();
            draw(b, rect(0.0, 0.0, 5.0, 5.0));
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
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            clip(b, rect(50.0, 50.0, 100.0, 100.0));
            draw(b, rect(60.0, 60.0, 10.0, 10.0));
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

#[test]
fn cull_drops_drawrect_entirely_outside_active_clip() {
    // Two rect quads under the same clip: one inside, one fully
    // outside. Composer must skip emitting the outside one (the GPU
    // would scissor it, but skipping the `quads.push` saves CPU work).
    // Push/Pop pair still emits a single scissored group covering the
    // visible quad.
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(20.0, 20.0, 30.0, 30.0)); // inside
            draw(b, rect(200.0, 200.0, 30.0, 30.0)); // entirely outside
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1, "outside-clip rect must be culled");
    assert_eq!(buf.groups.len(), 1);
    assert!(buf.groups[0].scissor.is_some());
}

#[test]
fn cull_drops_drawtext_entirely_outside_active_clip() {
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(10.0, 10.0, 50.0, 20.0)); // inside
            text(b, rect(300.0, 300.0, 50.0, 20.0)); // outside
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1, "outside-clip text run must be culled");
}

#[test]
fn cull_keeps_drawrect_partially_inside_active_clip() {
    // Partial overlap counts — anything that could light a pixel keeps
    // its quad. Only fully-disjoint draws are dropped.
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(80.0, 80.0, 50.0, 50.0)); // straddles the clip
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1, "straddling rect must still emit");
}

#[test]
fn cull_without_active_clip_keeps_nonzero_viewport_bounds() {
    let buf = run(
        |b, _arena| {
            draw(b, rect(-10.0, -10.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.groups.len(), 1);
}

#[test]
fn cull_drops_drawmesh_entirely_outside_active_clip() {
    // Mesh now gets the same active-clip cull every other shape draw
    // performs. Two meshes under one clip: inside emits a row, fully
    // outside is culled.
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            mesh(b, rect(10.0, 10.0, 30.0, 30.0)); // inside
            mesh(b, rect(200.0, 200.0, 30.0, 30.0)); // outside the clip
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.meshes.len(), 1, "outside-clip mesh must be culled");
}

#[test]
fn cull_handles_culled_text_then_quad_split() {
    // The text-then-quad split rule lives in `GroupBuilder`. A culled
    // text run must NOT flag `last_was_text`, otherwise the next quad
    // would force a spurious group flush. Verify by drawing
    // [text-out, rect-in, rect-in] under the same clip — they should
    // share one group with both rects in it (no spurious split).
    let buf = run(
        |b, _arena| {
            clip(b, rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(300.0, 300.0, 50.0, 20.0)); // culled
            draw(b, rect(10.0, 10.0, 30.0, 30.0));
            draw(b, rect(50.0, 50.0, 30.0, 30.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 0);
    assert_eq!(buf.quads.len(), 2);
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
            clip(b, rect(0.0, 0.0, 50.0, 50.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.groups.is_empty());
}

/// Composer plumbing for rounded clip: radius + rect ride on the
/// emitted `DrawGroup` as a one-entry mask chain, scaled by DPR.
/// Inheritance verified in the same fixture: a `Rect` clip pushed
/// inside the `Rounded` parent must inherit the parent's chain so
/// children stay stencil-tested against the active mask. Without
/// inheritance, inner draws would land at `stencil_ref=0` over
/// `stencil=1` pixels and disappear.
#[test]
fn push_clip_rounded_lands_radius_on_group_and_inherits_through_rect() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, rect(10.0, 20.0, 100.0, 80.0), Corners::all(8.0));
            // Tier 1: direct draw under the rounded clip.
            draw(b, rect(20.0, 30.0, 40.0, 40.0));
            // Tier 2: nest a plain rect clip — children of THIS clip
            // must still inherit the rounded info from the ancestor.
            clip(b, rect(30.0, 40.0, 40.0, 30.0));
            draw(b, rect(35.0, 45.0, 10.0, 10.0));
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
    assert_eq!(outer_r.mask_rect.min, glam::Vec2::new(20.0, 40.0));
    assert_eq!(outer_r.mask_rect.size, Size::new(200.0, 160.0));
    assert_eq!(outer.scissor, Some(URect::new(20, 40, 200, 160)));

    // Inheritance: inner Rect clip carries the SAME chain as the
    // outer parent (span-identical — the mask geometry is the
    // ancestor's, scissor is narrowed independently).
    assert_eq!(
        inner.rounded_clips, outer.rounded_clips,
        "inner group inherits parent's mask chain verbatim"
    );
    // DPR=2: rect (30,40,40,30) → (60,80,80,60), clamped to outer.
    assert_eq!(inner.scissor, Some(URect::new(60, 80, 80, 60)));
}

/// Nested rounded clips STACK: the child group's chain lists both
/// masks in outer→inner order (the ancestor's corner cutouts keep
/// clipping child content — a fresh single mask would paint the child
/// square over them), and a rect clip nested below inherits the full
/// depth-2 chain. Hand-computed at DPR 1: outer = (10,10,200,200) r8,
/// inner = (20,20,100,100) r4.
#[test]
fn push_clip_rounded_nested_builds_outer_inner_chain() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, rect(10.0, 10.0, 200.0, 200.0), Corners::all(8.0));
            draw(b, rect(20.0, 20.0, 40.0, 40.0));
            clip_rounded(b, rect(20.0, 20.0, 100.0, 100.0), Corners::all(4.0));
            draw(b, rect(30.0, 30.0, 20.0, 20.0));
            clip(b, rect(30.0, 30.0, 50.0, 50.0));
            draw(b, rect(35.0, 35.0, 10.0, 10.0));
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
    assert_eq!(outer[0].mask_rect, rect(10.0, 10.0, 200.0, 200.0));
    assert_eq!(outer[0].corners.as_array()[0], 8.0);

    let nested = chain(1);
    assert_eq!(nested.len(), 2, "nested rounded stacks on the ancestor");
    assert_eq!(
        nested[0], outer[0],
        "chain lists the ancestor first (outer→inner)"
    );
    assert_eq!(nested[1].mask_rect, rect(20.0, 20.0, 100.0, 100.0));
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
            draw(buffer, rect(100.0, 100.0, 20.0, 20.0));
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

/// Re-pushing the innermost rounded clip verbatim (same rect + radii)
/// adds no chain depth and — like the redundant rect Push/Pop — is a
/// full no-op: no batch split, no group flush.
#[test]
fn push_clip_rounded_redundant_identical_push_adds_no_depth() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, rect(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, rect(20.0, 20.0, 20.0, 20.0));
            clip_rounded(b, rect(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, rect(50.0, 50.0, 20.0, 20.0));
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

/// Regression: when a rounded clip partially leaves the viewport, the
/// rasterizer scissor clamps to viewport bounds — but the mask SDF
/// must keep seeing the rect's **true** edges. Otherwise corner
/// curves "slide inward" into visible pixels, and rounded clipping
/// bleeds inside the control while resizing the window.
#[test]
fn push_clip_rounded_mask_rect_is_unclamped_to_viewport() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, rect(-50.0, -20.0, 200.0, 100.0), Corners::all(8.0));
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(120, 60)),
    );
    let chain = &buf.rounded_clips[buf.groups[0].rounded_clips.range()];
    let r = chain[0];
    // Mask rect keeps the off-screen origin (-50,-20) and full size
    // (200,100) — the SDF needs the rect's full geometry.
    assert_eq!(r.mask_rect.min, Vec2::new(-50.0, -20.0));
    assert_eq!(r.mask_rect.size, Size::new(200.0, 100.0));
    // Scissor clamps to viewport so the GPU rasterizer rejects
    // off-screen pixels.
    assert_eq!(buf.groups[0].scissor, Some(URect::new(0, 0, 120, 60)));
}

#[test]
fn push_clip_rect_emits_no_rounded_data() {
    let buf = run(
        |b, _arena| {
            clip(b, rect(10.0, 20.0, 100.0, 80.0));
            draw(b, rect(20.0, 30.0, 10.0, 10.0));
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
            draw(b, rect(-40.0, 10.0, 10.0, 10.0));
            mesh(b, rect(10.0, -40.0, 10.0, 10.0));
            image(b, rect(240.0, 10.0, 10.0, 10.0));
            curve(b, rect(10.0, 240.0, 10.0, 10.0));
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
