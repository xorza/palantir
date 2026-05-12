use super::super::cmd_buffer::{DrawPolylinePayload, RenderCmdBuffer};
use super::Composer;
use crate::layout::types::{display::Display, span::Span};
use crate::primitives::{
    brush::Brush, color::Color, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
    transform::TranslateScale, urect::URect,
};
use crate::renderer::render_buffer::RenderBuffer;
use crate::shape::{ColorMode, ColorModeBits, LineCap, LineCapBits, LineJoin, LineJoinBits};
use crate::text::TextCacheKey;
use glam::{UVec2, Vec2};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_rect(
        r,
        Corners::default(),
        &Brush::Solid(Color::rgb(1.0, 1.0, 1.0)),
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
    let mut out = RenderBuffer::default();
    composer.compose(&buffer, *display, &mut out);
    out
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
                &Brush::Solid(Color::rgb(1.0, 1.0, 1.0)),
                Stroke::solid(Color::rgb(0.0, 0.0, 0.0), 1.5),
            );
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.radius.tl, 16.0);
    assert_eq!(q.stroke_width, 3.0);
}

/// Solid `Brush::Solid` panel: composer emits a Quad with
/// `fill_kind = BRUSH_KIND_SOLID = 0`, `fill_lut_row = 0` (sentinel
/// for "no gradient"), and the fill colour pass-through. Catches a
/// regression that accidentally sets `fill_kind = 1` on solid quads.
#[test]
fn compose_solid_brush_emits_kind_zero_quad() {
    use crate::primitives::brush::LinearGradient;
    use crate::primitives::color::Srgb8;
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        &Brush::Solid(Color::rgb(0.5, 0.5, 0.5)),
        Stroke::ZERO,
    );
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(&buffer, params(1.0, UVec2::new(100, 100)), &mut out);
    let q = &out.quads[0];
    assert!(q.fill_kind.is_solid(), "solid quad must carry kind=solid");
    assert_eq!(
        q.fill_lut_row,
        crate::renderer::gradient_atlas::LutRow::FALLBACK,
        "solid quad has no LUT row",
    );
    assert_eq!(
        q.fill_axis,
        crate::primitives::brush::FillAxis::ZERO,
        "solid quad axis is zeroed",
    );
    // Suppress unused-import warning for the gradient helper used in
    // the linear test below.
    let _ = LinearGradient::two_stop(0.0, Srgb8::WHITE, Srgb8::BLACK);
}

/// `Brush::Linear` panel: composer registers the gradient with the
/// atlas (returns a non-zero row), packs the row id into Quad's
/// `fill_lut_row`, copies the axis vector, and sets `fill_kind = 1`
/// with the spread mode in bits 8..16.
#[test]
fn compose_linear_brush_emits_kind_one_with_atlas_row() {
    use crate::primitives::brush::{LinearGradient, Spread};
    use crate::primitives::color::Srgb8;
    let g = LinearGradient::two_stop(0.0, Srgb8::WHITE, Srgb8::BLACK).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        &Brush::Linear(g),
        Stroke::ZERO,
    );
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(&buffer, params(1.0, UVec2::new(100, 100)), &mut out);
    let q = &out.quads[0];
    assert!(
        q.fill_kind.is_gradient(),
        "linear quad carries gradient kind"
    );
    // Spread bits aren't exposed as a public accessor on FillKind; pin
    // identity via the matching constructor — same bit pattern reaches
    // the shader regardless.
    let expected_kind = crate::renderer::quad::FillKind::linear(Spread::Reflect);
    assert_eq!(q.fill_kind, expected_kind);
    assert!(q.fill_lut_row.0 >= 1, "linear quad must get a real row");
    assert_eq!(q.fill_axis, expected_axis);
}

/// Two quads referencing the same gradient share an atlas row.
/// Content-hash addressing keeps the bake step idempotent across
/// frames and across multiple emitting widgets.
#[test]
fn compose_repeated_linear_brush_shares_atlas_row() {
    use crate::primitives::brush::LinearGradient;
    use crate::primitives::color::Srgb8;
    let g = LinearGradient::two_stop(0.5, Srgb8::hex(0x336699), Srgb8::hex(0xddaa44));
    let mut buffer = RenderCmdBuffer::default();
    for _ in 0..3 {
        buffer.draw_rect(
            rect(0.0, 0.0, 10.0, 10.0),
            Corners::default(),
            &Brush::Linear(g),
            Stroke::ZERO,
        );
    }
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(&buffer, params(1.0, UVec2::new(100, 100)), &mut out);
    let rows: Vec<_> = out.quads.iter().map(|q| q.fill_lut_row).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    assert!(rows[0].0 >= 1);
}

/// Pin: text-run scale snaps to the 2.5% ladder so continuous zoom
/// produces stable glyphon cache keys across adjacent frames. Quads
/// (next test) intentionally do not snap — only text quantizes.
#[test]
fn compose_snaps_text_scale_to_discrete_steps() {
    // 1.013 is between 1.000 and 1.025; rounds to 1.025.
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(1.013));
            text(b, rect(0.0, 0.0, 50.0, 20.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1);
    let s = buf.texts[0].scale;
    assert!(
        (s - 1.025).abs() < 1e-5,
        "1.013 must snap to 1.025, got {s}",
    );
}

/// Pin: a quad pushed under the same fractional transform keeps its
/// continuous scale — only text snaps. Otherwise a zoomed layout
/// would visibly jitter as quad sizes step alongside font cache keys.
#[test]
fn compose_keeps_quad_scale_continuous_under_zoom() {
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(1.013));
            draw(b, rect(0.0, 0.0, 100.0, 50.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    // 100*1.013 = 101.3; 50*1.013 = 50.65 — preserved, not snapped.
    assert!((buf.quads[0].rect.size.w - 101.3).abs() < 1e-4);
    assert!((buf.quads[0].rect.size.h - 50.65).abs() < 1e-3);
}

#[test]
fn compose_propagates_transform_scale_to_text_runs() {
    // A `TranslateScale(_, 2.0)` ancestor must surface on the emitted
    // TextRun.scale so glyphon paints proportionally larger glyphs.
    // Without this the rect stretches but the glyph rasters stay at
    // the originally-shaped size — visible as text "not zooming" inside
    // a zoomed Scroll viewport.
    let buf = run(
        |b| {
            b.push_transform(TranslateScale::from_scale(2.0));
            text(b, rect(0.0, 0.0, 50.0, 20.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(buf.texts[0].scale, 2.0);
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
        |b| {
            b.push_clip(rect(0.0, 0.0, 200.0, 200.0));
            draw(b, rect(0.0, 0.0, 100.0, 28.0)); // node A bg
            text(b, rect(4.0, 4.0, 90.0, 20.0)); //  node A label
            // Redundant nested clip — same rect, no narrowing.
            b.push_clip(rect(0.0, 0.0, 200.0, 200.0));
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
        |b| {
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
    assert_eq!(buf.groups[0].texts, Span::new(0, 5));
}

/// Pin: when a later quad DOES overlap a prior text (the node-editor
/// case — node B's chrome lands on node A's label), the composer
/// must flush so paint order is preserved. Same fixture shape as the
/// row-batching test but the second row's chrome is offset to land
/// on the first row's label.
#[test]
fn compose_flushes_when_later_quad_overlaps_prior_text() {
    let buf = run(
        |b| {
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

// ---------- Text batch coalescing across groups -------------------

/// Pin: two adjacent rows where each row sits in its own scissor
/// (a clipped panel per row) coalesce their text into ONE batch even
/// though they're in different groups. Saves a glyphon prepare +
/// render per extra row — the bulk of the savings from text batching.
#[test]
fn compose_coalesces_text_across_distinct_scissor_groups() {
    let buf = run(
        |b| {
            b.push_clip(rect(0.0, 0.0, 100.0, 30.0));
            draw(b, rect(0.0, 0.0, 100.0, 28.0));
            text(b, rect(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            b.push_clip(rect(0.0, 40.0, 100.0, 30.0));
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

/// Pin: a rounded-clip change splits the text batch even when text
/// across the change wouldn't otherwise overlap. Different rounded
/// clips → different stencil refs at render time; one merged prepare
/// would mis-clip text under one of them.
#[test]
fn compose_rounded_clip_change_splits_text_batch() {
    let buf = run(
        |b| {
            b.push_clip_rounded(rect(0.0, 0.0, 100.0, 30.0), Corners::all(4.0));
            text(b, rect(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            b.push_clip_rounded(rect(0.0, 40.0, 100.0, 30.0), Corners::all(8.0));
            text(b, rect(4.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.text_batches.len(), 2, "rounded change must split batch");
}

/// Pin: a mesh (here, a polyline lowering to a mesh) recorded between
/// two text runs splits the batch. Mesh paints over text by kind
/// order; if it weren't a split, the merged batch's text would emit
/// at end-of-batch, *after* the mesh, breaking that ordering.
#[test]
fn compose_mesh_between_texts_splits_text_batch() {
    let buf = run(
        |b| {
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            // Stuff a 2-point polyline into the arena and record it.
            let p_start = b.shape_payloads.polyline_points.len() as u32;
            b.shape_payloads.polyline_points.push(Vec2::new(0.0, 25.0));
            b.shape_payloads
                .polyline_points
                .push(Vec2::new(100.0, 25.0));
            let c_start = b.shape_payloads.polyline_colors.len() as u32;
            b.shape_payloads.polyline_colors.push(Color::WHITE);
            b.draw_polyline(DrawPolylinePayload {
                bbox: rect(0.0, 25.0, 100.0, 0.0),
                width: 1.0,
                points_start: p_start,
                points_len: 2,
                colors_start: c_start,
                colors_len: 1,
                color_mode: ColorModeBits::new(ColorMode::Single),
                cap: LineCapBits::new(LineCap::Butt),
                join: LineJoinBits::new(LineJoin::Miter),
                ..bytemuck::Zeroable::zeroed()
            });
            text(b, rect(0.0, 40.0, 100.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.text_batches.len(),
        2,
        "mesh between texts must split the batch",
    );
}

/// Pin: a quad that overlaps prior batch text closes the batch — the
/// merged batch would otherwise paint that text over the occluding
/// quad. Two groups, two text batches; quad in the middle.
#[test]
fn compose_quad_overlap_with_prior_batch_text_splits_batch() {
    let buf = run(
        |b| {
            text(b, rect(0.0, 0.0, 100.0, 30.0)); // text A
            // Push a clip to force a fresh group; quad inside overlaps text A.
            b.push_clip(rect(0.0, 0.0, 200.0, 200.0));
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
