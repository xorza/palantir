use super::super::cmd_buffer::RenderCmdBuffer;
use super::Composer;
use crate::layout::types::{display::Display, span::Span};
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
    transform::TranslateScale, urect::URect,
};
use crate::renderer::render_buffer::RenderBuffer;
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_rect(
        r,
        Corners::default(),
        Color::rgb(1.0, 1.0, 1.0),
        Stroke::ZERO,
    );
}

fn text(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_text(r, Color::WHITE, TextCacheKey::INVALID);
}

fn params(scale: f32, physical: UVec2) -> Display {
    Display {
        physical,
        scale_factor: scale,
        pixel_snap: false,
    }
}

fn run(build: impl FnOnce(&mut RenderCmdBuffer), display: &Display) -> RenderBuffer {
    let mut buffer = RenderCmdBuffer::default();
    build(&mut buffer);
    let mut composer = Composer::default();
    composer.compose(&buffer, display);
    std::mem::take(&mut composer.buffer)
}

#[test]
fn compose_with_no_clip_emits_one_unscissored_group() {
    let buf = run(
        |b| {
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

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            b.push_clip(rect(50.0, 50.0, 100.0, 100.0));
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
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 100, 100));
    assert_eq!(buf.groups[1].quads, Span::new(1, 2));

    assert!(buf.groups[2].scissor.is_none());
    assert_eq!(buf.groups[2].quads, Span::new(3, 1));
}

#[test]
fn compose_intersects_nested_clips() {
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            b.push_clip(rect(50.0, 50.0, 100.0, 100.0));
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
    assert_eq!((s.x, s.y, s.w, s.h), (50, 50, 50, 50));
}

#[test]
fn cull_drops_drawrect_entirely_outside_active_clip() {
    // Two `DrawRect`s under the same clip: one inside, one fully
    // outside. Composer must skip emitting the outside one (the GPU
    // would scissor it, but skipping the `quads.push` saves CPU work).
    // Push/Pop pair still emits a single scissored group covering the
    // visible quad.
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
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
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
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
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(80.0, 80.0, 50.0, 50.0)); // straddles the clip
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1, "straddling rect must still emit");
}

#[test]
fn cull_does_not_apply_without_active_clip() {
    // No `PushClip` ⇒ no scissor active. Even far-offscreen draws
    // emit; the GPU's viewport scissor handles culling. Pin so a
    // future tightening doesn't silently start dropping unscissored
    // draws.
    let buf = run(
        |b| {
            draw(b, rect(1000.0, 1000.0, 50.0, 50.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
}

#[test]
fn cull_handles_culled_text_then_quad_split() {
    // The text-then-quad split rule lives in `GroupBuilder`. A culled
    // text run must NOT flag `last_was_text`, otherwise the next quad
    // would force a spurious group flush. Verify by drawing
    // [text-out, rect-in, rect-in] under the same clip — they should
    // share one group with both rects in it (no spurious split).
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
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
        |b| {
            b.push_clip(rect(0.0, 0.0, 50.0, 50.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(buf.quads.is_empty());
    assert!(buf.groups.is_empty());
}

/// Composer plumbing for rounded clip: radius + rect ride on the
/// emitted `DrawGroup`, scaled by DPR. Inheritance verified in the
/// same fixture: a `Rect` clip pushed inside the `Rounded` parent
/// must inherit the parent's `rounded_clip` so children stay
/// stencil-tested against the active mask. Without inheritance,
/// inner draws would land at `stencil_ref=0` over `stencil=1`
/// pixels and disappear.
#[test]
fn push_clip_rounded_lands_radius_on_group_and_inherits_through_rect() {
    let buf = run(
        |b| {
            b.push_clip_rounded(rect(10.0, 20.0, 100.0, 80.0), Corners::all(8.0));
            // Tier 1: direct draw under the rounded clip.
            draw(b, rect(20.0, 30.0, 40.0, 40.0));
            // Tier 2: nest a plain rect clip — children of THIS clip
            // must still inherit the rounded info from the ancestor.
            b.push_clip(rect(30.0, 40.0, 40.0, 30.0));
            draw(b, rect(35.0, 45.0, 10.0, 10.0));
            b.pop_clip();
            b.pop_clip();
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert!(buf.has_rounded_clip);
    assert_eq!(
        buf.groups.len(),
        2,
        "two groups: outer rounded scissor, inner rect scissor"
    );

    let outer = &buf.groups[0];
    let inner = &buf.groups[1];

    let outer_r = outer
        .rounded_clip
        .expect("outer rounded data must ride on group");
    // DPR=2 → radius doubles 8→16, rect (10,20,100,80) → (20,40,200,160).
    assert_eq!(outer_r.radius.tl, 16.0);
    assert_eq!(outer_r.mask_rect.min, glam::Vec2::new(20.0, 40.0));
    assert_eq!(
        outer_r.mask_rect.size,
        crate::primitives::size::Size::new(200.0, 160.0)
    );
    assert_eq!(outer.scissor, Some(URect::new(20, 40, 200, 160)));

    // Inheritance: inner Rect clip carries the SAME rounded data as
    // the outer parent (rect AND radius — the mask geometry is the
    // ancestor's, scissor is narrowed independently).
    let inner_r = inner
        .rounded_clip
        .expect("inner rect clip inside rounded ancestor inherits rounded data");
    assert_eq!(inner_r, outer_r, "inner group inherits parent's mask data");
    // DPR=2: rect (30,40,40,30) → (60,80,80,60), clamped to outer.
    assert_eq!(inner.scissor, Some(URect::new(60, 80, 80, 60)));
}

/// Regression: when a rounded clip partially leaves the viewport, the
/// rasterizer scissor clamps to viewport bounds — but the mask SDF
/// must keep seeing the rect's **true** edges. Otherwise corner
/// curves "slide inward" into visible pixels, and rounded clipping
/// bleeds inside the control while resizing the window.
#[test]
fn push_clip_rounded_mask_rect_is_unclamped_to_viewport() {
    let buf = run(
        |b| {
            b.push_clip_rounded(rect(-50.0, -20.0, 200.0, 100.0), Corners::all(8.0));
            draw(b, rect(0.0, 0.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(120, 60)),
    );
    let r = buf.groups[0].rounded_clip.expect("rounded data on group");
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
        |b| {
            b.push_clip(rect(10.0, 20.0, 100.0, 80.0));
            draw(b, rect(20.0, 30.0, 10.0, 10.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert!(!buf.has_rounded_clip);
    assert!(buf.groups[0].rounded_clip.is_none());
}

#[test]
fn compose_scales_rects_for_dpr() {
    let buf = run(
        |b| draw(b, rect(10.0, 20.0, 30.0, 40.0)),
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(q.rect.size, Size::new(60.0, 80.0));
}

#[test]
fn intersect_disjoint_yields_zero_size() {
    let a = URect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    let b = URect {
        x: 100,
        y: 100,
        w: 10,
        h: 10,
    };
    // The composer uses `URect::clamp_to` for child↔parent scissor
    // intersection — disjoint rects collapse to a zero-sized result.
    let r = b.clamp_to(a);
    assert_eq!(r.w, 0);
    assert_eq!(r.h, 0);
}

#[test]
fn compose_translates_under_push_transform() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_translation(Vec2::new(100.0, 50.0)));
            draw(b, rect(10.0, 20.0, 30.0, 40.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(110.0, 70.0));
    assert_eq!(q.rect.size, Size::new(30.0, 40.0));
}

#[test]
fn compose_scales_radius_and_stroke_under_transform() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.draw_rect(
                rect(0.0, 0.0, 50.0, 50.0),
                Corners::all(8.0),
                Color::rgb(1.0, 1.0, 1.0),
                Stroke {
                    width: 1.5,
                    color: Color::rgb(0.0, 0.0, 0.0),
                },
            );
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.radius.tl, 16.0);
    assert_eq!(q.stroke.width, 3.0);
}

#[test]
fn compose_composes_nested_transforms() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.push_transform(TranslateScale::from_translation(Vec2::new(10.0, 0.0)));
            draw(b, rect(5.0, 0.0, 10.0, 10.0));
            b.pop_transform();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(30.0, 0.0));
    assert_eq!(q.rect.size, Size::new(20.0, 20.0));
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.push_clip(rect(10.0, 10.0, 20.0, 20.0));
            draw(b, rect(15.0, 15.0, 5.0, 5.0));
            b.pop_clip();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.x, s.y, s.w, s.h), (20, 20, 40, 40));
}

/// Pin: a `Quad → Text → Quad` paint sequence inside a single scissor
/// produces TWO groups so the second quad renders *after* the text.
/// Without this split, `submit` batches both quads together and the
/// text always paints on top — which is the bug the `text z-order`
/// showcase tab exposes.
#[test]
fn compose_splits_group_on_text_to_quad_transition() {
    let buf = run(
        |b| {
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
    // First group: quad #0 + text #0.
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.groups[0].texts, Span::new(0, 1));
    // Second group: quad #1 only — renders after group 0's text.
    assert_eq!(buf.groups[1].quads, Span::new(1, 1));
    assert_eq!(buf.groups[1].texts, Span::new(1, 0));
}

/// Pin: consecutive `Text → Text` should NOT split (both go into the
/// same group). Only `Text → Quad` triggers a flush. Otherwise a
/// header-then-body label pair produces two groups for nothing.
#[test]
fn compose_does_not_split_consecutive_texts() {
    let buf = run(
        |b| {
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
    assert_eq!(buf.groups[0].texts, Span::new(0, 2));
}

/// Pin: `Quad → Quad → Text` fits in one group. The text comes after
/// both quads and renders on top of both — the common case (button
/// background + button stroke + label).
#[test]
fn compose_keeps_quads_then_text_in_one_group() {
    let buf = run(
        |b| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            draw(b, rect(2.0, 2.0, 96.0, 96.0));
            text(b, rect(10.0, 10.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
    assert_eq!(buf.groups[0].texts, Span::new(0, 1));
}
