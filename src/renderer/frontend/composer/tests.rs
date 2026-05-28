use super::super::cmd_buffer::{DrawMeshPayload, DrawPolylinePayload, RenderCmdBuffer};
use super::Composer;
use crate::common::frame_arena::FrameArenaInner;
use crate::layout::types::display::Display;
use crate::primitives::span::Span;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
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
        crate::renderer::frontend::cmd_buffer::BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
        Stroke::ZERO.into(),
    );
}

fn text(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_text(r, Color::WHITE.into(), TextCacheKey::INVALID);
}

fn params(scale: f32, physical: UVec2) -> Display {
    Display {
        physical,
        scale_factor: scale,
        pixel_snap: false,
    }
}

fn run(
    build: impl FnOnce(&mut RenderCmdBuffer, &mut FrameArenaInner),
    display: &Display,
) -> RenderBuffer {
    let mut buffer = RenderCmdBuffer::default();
    let mut arena = FrameArenaInner::default();
    build(&mut buffer, &mut arena);
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(&buffer, &mut arena, *display, &mut out);
    out
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

#[test]
fn compose_with_clip_groups_inner_draws_under_scissor() {
    let buf = run(
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
            draw(b, rect(1000.0, 1000.0, 50.0, 50.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
}

fn mesh(buf: &mut RenderCmdBuffer, bbox: Rect) {
    // 3 verts / 3 indices + opaque tint clears `DrawMeshPayload::is_noop`
    // so the cmd reaches the composer.
    buf.draw_mesh(DrawMeshPayload {
        bbox,
        origin: Vec2::ZERO,
        tint: Color::WHITE.into(),
        v_start: 0,
        v_len: 3,
        i_start: 0,
        i_len: 3,
        ..bytemuck::Zeroable::zeroed()
    });
}

#[test]
fn cull_drops_drawmesh_entirely_outside_active_clip() {
    // Mesh now gets the same active-clip cull every other shape draw
    // performs. Two meshes under one clip: inside emits a row, fully
    // outside is culled.
    let buf = run(
        |b, _arena| {
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            mesh(b, rect(10.0, 10.0, 30.0, 30.0)); // inside
            mesh(b, rect(200.0, 200.0, 30.0, 30.0)); // outside the clip
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.meshes.rows.len(), 1, "outside-clip mesh must be culled");
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
        |b, _arena| {
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
        |b, _arena| {
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
    assert_eq!(outer_r.corners.as_array()[0], 16.0);
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| draw(b, rect(10.0, 20.0, 30.0, 40.0)),
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
        |b, _arena| {
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
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.draw_rect(
                rect(0.0, 0.0, 50.0, 50.0),
                Corners::all(8.0),
                crate::renderer::frontend::cmd_buffer::BrushSource::Solid(
                    Color::rgb(1.0, 1.0, 1.0).into(),
                ),
                Stroke::solid(Color::rgb(0.0, 0.0, 0.0), 1.5).into(),
            );
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.corners.as_array()[0], 16.0);
    assert_eq!(q.stroke_width, 3.0);
}

/// Solid `Brush::Solid` panel: composer emits a Quad with
/// `fill_kind = BRUSH_KIND_SOLID = 0`, `fill_lut_row = 0` (sentinel
/// for "no gradient"), and the fill colour pass-through. Catches a
/// regression that accidentally sets `fill_kind = 1` on solid quads.
#[test]
fn compose_solid_brush_emits_kind_zero_quad() {
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        BrushSource::Solid(Color::rgb(0.5, 0.5, 0.5).into()),
        Stroke::ZERO.into(),
    );
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(
        &buffer,
        &mut FrameArenaInner::default(),
        params(1.0, UVec2::new(100, 100)),
        &mut out,
    );
    let q = &out.quads[0];
    assert_eq!(
        q.fill_kind,
        crate::renderer::quad::FillKind::SOLID,
        "solid quad must carry kind=solid",
    );
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
}

/// `Brush::Linear` panel: lowering registers the gradient with the
/// atlas (returns a non-zero row), packs the row + axis + kind into
/// the cmd-buffer payload; composer pipes the row straight through
/// to the emitted Quad.
#[test]
fn compose_linear_brush_emits_kind_one_with_atlas_row() {
    use crate::forest::shapes::record::LoweredGradient;
    use crate::primitives::brush::{LinearGradient, Spread};
    use crate::primitives::color::ColorU8;
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    use crate::renderer::gradient_atlas::GradientAtlas;
    use crate::renderer::quad::FillKind;
    let g =
        LinearGradient::two_stop(0.0, ColorU8::WHITE, ColorU8::BLACK).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let atlas = GradientAtlas::default();
    let row = atlas.register_stops(&g.stops, g.interp);
    let lowered = LoweredGradient {
        axis: expected_axis,
        row,
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        BrushSource::Gradient(lowered),
        Stroke::ZERO.into(),
    );
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(
        &buffer,
        &mut FrameArenaInner::default(),
        params(1.0, UVec2::new(100, 100)),
        &mut out,
    );
    let q = &out.quads[0];
    assert_eq!(q.fill_kind, FillKind::linear(Spread::Reflect));
    assert!(q.fill_lut_row.0 >= 1, "linear quad must get a real row");
    assert_eq!(q.fill_axis, expected_axis);
}

/// Two quads referencing the same gradient share an atlas row.
/// Content-hash addressing keeps the bake step idempotent across
/// frames and across multiple emitting widgets.
#[test]
fn compose_repeated_linear_brush_shares_atlas_row() {
    use crate::forest::shapes::record::LoweredGradient;
    use crate::primitives::brush::LinearGradient;
    use crate::primitives::color::ColorU8;
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    use crate::renderer::gradient_atlas::GradientAtlas;
    use crate::renderer::quad::FillKind;
    let g = LinearGradient::two_stop(0.5, ColorU8::hex(0x336699), ColorU8::hex(0xddaa44));
    let atlas = GradientAtlas::default();
    let lowered = LoweredGradient {
        axis: g.axis(),
        row: atlas.register_stops(&g.stops, g.interp),
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = RenderCmdBuffer::default();
    for _ in 0..3 {
        buffer.draw_rect(
            rect(0.0, 0.0, 10.0, 10.0),
            Corners::default(),
            BrushSource::Gradient(lowered),
            Stroke::ZERO.into(),
        );
    }
    let mut composer = Composer::default();
    let mut out = RenderBuffer::default();
    composer.compose(
        &buffer,
        &mut FrameArenaInner::default(),
        params(1.0, UVec2::new(100, 100)),
        &mut out,
    );
    let rows: Vec<_> = out.quads.iter().map(|q| q.fill_lut_row).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    assert!(rows[0].0 >= 1);
}

/// Pin: text-run scale snaps to the additive 2.5% ladder so continuous
/// zoom produces stable glyphon cache keys across adjacent frames.
/// Quads (next test) intentionally do not snap — only text quantizes.
#[test]
fn compose_snaps_text_scale_to_discrete_steps() {
    // 1.013 is between 1.000 and 1.025; rounds to 1.025.
    let buf = run(
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
        |b, _arena| {
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
            b.push_clip(rect(40.0, 40.0, 20.0, 20.0));
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
        .find(|tb| tb.scissor.w == 20)
        .expect("expected a batch with 20px-wide scissor");
    assert_eq!(strict.scissor.w, 20);
    assert_eq!(strict.scissor.h, 20);
}

/// Pin: two strict runs whose clips happen to be IDENTICAL rects can
/// coalesce into one batch — the GPU scissor matches both. Important
/// for repeated strict clips (e.g. a column of clipped numeric inputs
/// all the same width).
#[test]
fn compose_strict_text_with_matching_clip_coalesces() {
    let clip = rect(40.0, 40.0, 20.0, 20.0);
    let buf = run(
        |b, _arena| {
            b.push_clip(clip);
            text(b, rect(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
            b.push_clip(clip);
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
/// clips → different stencil refs at render time; one merged prepare
/// would mis-clip text under one of them.
#[test]
fn compose_rounded_clip_change_splits_text_batch() {
    let buf = run(
        |b, _arena| {
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
        |b, arena| {
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            // Stuff a 2-point polyline into the frame arena and record it.
            let p_start = arena.polyline_points.len() as u32;
            arena.polyline_points.push(Vec2::new(0.0, 25.0));
            arena.polyline_points.push(Vec2::new(100.0, 25.0));
            let c_start = arena.polyline_colors.len() as u32;
            arena.polyline_colors.push(Color::WHITE.into());
            b.draw_polyline(DrawPolylinePayload {
                bbox: rect(0.0, 25.0, 100.0, 0.0),
                origin: Vec2::ZERO,
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
    // Phase 2 structural: a polyline lowering produces a MeshBatch
    // parallel to its group's meshes span (1:1 mapping today).
    assert_eq!(
        buf.mesh_batches.len(),
        1,
        "polyline must contribute one mesh batch",
    );
    let mb = buf.mesh_batches[0];
    assert_eq!(
        mb.meshes.len, 1,
        "mesh batch covers exactly the one polyline draw",
    );
    assert_eq!(
        mb.last_group as usize,
        buf.groups.len() - 1,
        "mesh batch anchors at the group that emitted the polyline",
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
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            mesh(b, rect(200.0, 200.0, 30.0, 30.0)); // outside the clip → culled
            text(b, rect(0.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.meshes.rows.len(), 0, "the mesh must be culled");
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

#[test]
fn compose_emits_image_batch_for_drawimage() {
    use super::super::cmd_buffer::DrawImagePayload;
    let buf = run(
        |b, _arena| {
            b.draw_image(DrawImagePayload {
                rect: rect(10.0, 20.0, 30.0, 40.0),
                uv_min: glam::Vec2::ZERO,
                uv_size: glam::Vec2::ONE,
                tint: Color::WHITE.into(),
                handle: 0xc0ffee,
                ..bytemuck::Zeroable::zeroed()
            });
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.rows.len(), 1, "one image draw");
    assert_eq!(buf.images.rows.len(), 1, "one image instance");
    assert_eq!(buf.image_batches.len(), 1, "one image batch");
    assert_eq!(buf.image_batches[0].images, Span::new(0, 1));
    assert_eq!(buf.images.rows.handle()[0].id, 0xc0ffee);
    // Physical-px rect = logical * scale (no snap in `params`).
    assert_eq!(
        buf.images.rows.instance()[0].rect,
        rect(20.0, 40.0, 60.0, 80.0)
    );
    // Composer must forward the encoder's UV crop verbatim — a Zero
    // UV size means "sample one texel forever" and silently paints
    // every image as a uniform color (regression hunt: 2026-05).
    assert_eq!(buf.images.rows.instance()[0].uv_min, glam::Vec2::ZERO);
    assert_eq!(buf.images.rows.instance()[0].uv_size, glam::Vec2::ONE);
}

#[test]
fn compose_image_forwards_uv_crop_for_cover_fit() {
    use super::super::cmd_buffer::DrawImagePayload;
    let buf = run(
        |b, _arena| {
            b.draw_image(DrawImagePayload {
                rect: rect(0.0, 0.0, 100.0, 100.0),
                uv_min: glam::Vec2::new(0.25, 0.0),
                uv_size: glam::Vec2::new(0.5, 1.0),
                tint: Color::WHITE.into(),
                handle: 1,
                ..bytemuck::Zeroable::zeroed()
            });
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        buf.images.rows.instance()[0].uv_min,
        glam::Vec2::new(0.25, 0.0)
    );
    assert_eq!(
        buf.images.rows.instance()[0].uv_size,
        glam::Vec2::new(0.5, 1.0)
    );
}

#[test]
fn compose_forwards_tiled_flag_and_repeat_uv() {
    use super::super::cmd_buffer::DrawImagePayload;
    let buf = run(
        |b, _arena| {
            // Non-tiled draw: flag stays 0.
            b.draw_image(DrawImagePayload {
                rect: rect(0.0, 0.0, 50.0, 50.0),
                uv_min: glam::Vec2::ZERO,
                uv_size: glam::Vec2::ONE,
                tint: Color::WHITE.into(),
                handle: 1,
                tiled: 0,
                ..bytemuck::Zeroable::zeroed()
            });
            // Tiled draw: UV size > 1 (3×2 repeats) + flag 1.
            b.draw_image(DrawImagePayload {
                rect: rect(0.0, 0.0, 50.0, 50.0),
                uv_min: glam::Vec2::ZERO,
                uv_size: glam::Vec2::new(3.0, 2.0),
                tint: Color::WHITE.into(),
                handle: 2,
                tiled: 1,
                ..bytemuck::Zeroable::zeroed()
            });
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.rows.instance()[0].tiled, 0);
    assert_eq!(buf.images.rows.instance()[1].tiled, 1);
    assert_eq!(
        buf.images.rows.instance()[1].uv_size,
        glam::Vec2::new(3.0, 2.0)
    );
}

#[test]
fn compose_emits_one_curve_batch_per_scissor_group() {
    use crate::renderer::frontend::cmd_buffer::DrawCurvePayload;
    let buf = run(
        |b, _arena| {
            // Two curves under one (implicit) scissor group → must
            // batch into a single `CurveBatch`. That's the load-bearing
            // promise: one draw call per scissor group, no matter how
            // many curves the group contains.
            for offset in [0.0_f32, 50.0] {
                b.draw_curve(DrawCurvePayload {
                    bbox: rect(0.0, 0.0, 100.0, 100.0),
                    origin: Vec2::ZERO,
                    p0: Vec2::new(offset, 0.0),
                    p1: Vec2::new(offset + 10.0, 50.0),
                    p2: Vec2::new(offset + 90.0, 50.0),
                    p3: Vec2::new(offset + 100.0, 0.0),
                    color: Color::WHITE.into(),
                    width: 2.0,
                    ..bytemuck::Zeroable::zeroed()
                });
            }
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.curve_batches.len(), 1, "one batch per group");
    let batch = buf.curve_batches[0];
    assert_eq!(batch.last_group, 0);
    // Sub-instance count depends on adaptive subdivision, but both
    // curves contribute the *same* per-curve count (identical shape),
    // so the total must be ≥ 2 and even.
    assert!(batch.instances.len >= 2 && batch.instances.len.is_multiple_of(2));
    assert_eq!(
        buf.curves.len() as u32,
        batch.instances.len,
        "batch covers every emitted instance",
    );
}

#[test]
fn compose_splits_curve_batches_across_scissor_groups() {
    use crate::renderer::frontend::cmd_buffer::DrawCurvePayload;
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bbox: rect(0.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                p0: Vec2::new(0.0, 0.0),
                p1: Vec2::new(10.0, 50.0),
                p2: Vec2::new(90.0, 50.0),
                p3: Vec2::new(100.0, 0.0),
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
            });
            b.push_clip(rect(0.0, 0.0, 50.0, 200.0));
            b.draw_curve(DrawCurvePayload {
                bbox: rect(0.0, 0.0, 50.0, 50.0),
                origin: Vec2::ZERO,
                p0: Vec2::new(0.0, 0.0),
                p1: Vec2::new(5.0, 25.0),
                p2: Vec2::new(45.0, 25.0),
                p3: Vec2::new(50.0, 0.0),
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
            });
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.curve_batches.len(),
        2,
        "scissor change closes the open batch and opens a new one",
    );
    assert!(
        buf.curve_batches[0].last_group < buf.curve_batches[1].last_group,
        "batches anchor to monotonically increasing groups",
    );
}

#[test]
fn compose_threads_curve_fill_kind_and_lut_row_into_instances() {
    use crate::primitives::brush::Spread;
    use crate::renderer::frontend::cmd_buffer::DrawCurvePayload;
    use crate::renderer::gradient_atlas::LutRow;
    use crate::renderer::quad::FillKind;
    let buf = run(
        |b, _arena| {
            // Linear gradient curve: fill_kind low byte = 1, lut_row = 7.
            // Every sub-instance must carry the same fill_kind and row.
            b.draw_curve(DrawCurvePayload {
                bbox: rect(0.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                p0: Vec2::new(0.0, 0.0),
                p1: Vec2::new(10.0, 50.0),
                p2: Vec2::new(90.0, 50.0),
                p3: Vec2::new(100.0, 0.0),
                color: Color::TRANSPARENT.into(),
                width: 4.0,
                fill_kind: FillKind::linear(Spread::Pad),
                fill_lut_row: LutRow(7),
                ..bytemuck::Zeroable::zeroed()
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

// -- TextRectGrid --

#[test]
fn text_grid_empty_returns_no_overlap() {
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(10, 10, 50, 50)));
}

#[test]
fn text_grid_zero_area_input_is_ignored() {
    // Push: zero w/h rects don't enter the index (they can't
    // intersect anything anyway). Query: zero w/h queries
    // short-circuit to false.
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 0, 50));
    g.push(URect::new(10, 10, 50, 0));
    assert_eq!(g.rects.len(), 0, "zero-area pushes don't grow the index");
    g.push(URect::new(10, 10, 50, 50));
    assert!(!g.any_overlap(URect::new(10, 10, 0, 50)));
    assert!(!g.any_overlap(URect::new(10, 10, 50, 0)));
}

#[test]
fn text_grid_finds_within_single_tile() {
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 40, 20));
    // Hit: overlapping rect inside the same tile.
    assert!(g.any_overlap(URect::new(20, 15, 5, 5)));
    // Miss: disjoint rect inside the same tile.
    assert!(!g.any_overlap(URect::new(0, 0, 5, 5)));
    // Miss: disjoint rect in a different tile (far away).
    assert!(!g.any_overlap(URect::new(500, 500, 10, 10)));
}

#[test]
fn text_grid_finds_across_tile_boundaries() {
    // Tile size is 64. A rect spanning tile boundary registers into
    // multiple tiles; queries from either tile must hit.
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(60, 60, 20, 20));
    assert!(g.any_overlap(URect::new(60, 60, 4, 4)), "left tile hit");
    assert!(g.any_overlap(URect::new(76, 76, 4, 4)), "right tile hit");
    assert!(g.any_overlap(URect::new(64, 64, 1, 1)), "boundary tile hit");
}

#[test]
fn text_grid_matches_linear_scan_on_random_workload() {
    // Cross-check: for a synthetic workload, the grid agrees with a
    // flat linear scan across many queries. Catches regressions where
    // the tile-range math (off-by-one on edges, missing the
    // last-pixel tile) lets a query miss a registered rect.
    let mut g = super::TextRectGrid::default();
    let viewport = UVec2::new(800, 600);
    g.start_frame(viewport);
    // Tiles of 64 px in an 800x600 viewport — boundaries at
    // 0,64,128,…,768 → 13 cols × 10 rows = 130 tiles.
    let rects = [
        URect::new(0, 0, 10, 10),
        URect::new(60, 60, 20, 20), // spans 2x2 tiles
        URect::new(100, 100, 50, 50),
        URect::new(250, 80, 80, 40),
        URect::new(500, 400, 100, 100),
        URect::new(0, 500, 800, 30), // full-width strip
        URect::new(640, 0, 40, 600), // full-height strip
    ];
    for r in rects {
        g.push(r);
    }
    // Probe a grid of query rects and confirm grid ↔ linear scan
    // verdicts agree everywhere.
    for qy in (0..600).step_by(37) {
        for qx in (0..800).step_by(43) {
            let q = URect::new(qx, qy, 20, 20);
            let linear = rects.iter().any(|r| r.intersect(q).is_some());
            let grid = g.any_overlap(q);
            assert_eq!(linear, grid, "disagreement at q={q:?}");
        }
    }
}

#[test]
fn text_grid_clear_drops_all_rects() {
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
    g.clear();
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn text_grid_shrinks_viewport_without_visible_stale_state() {
    // start_frame is grow-only: a smaller-viewport frame reuses the
    // larger backing vector, but the active grid still answers
    // correctly. The previous frame's rects must NOT show up after
    // start_frame clears.
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    g.push(URect::new(1500, 1500, 40, 40)); // far outside the smaller viewport
    g.start_frame(UVec2::new(256, 256));
    // Stale rect from the 2048-viewport frame must be cleared even
    // though its physical tile index lives past the new grid.
    assert!(!g.any_overlap(URect::new(1500, 1500, 4, 4)));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn text_grid_start_frame_is_grow_only() {
    // Internal contract: shrinking the viewport doesn't free the tile
    // vector — it stays sized to the high-water mark so the
    // resize-arm benchmark (cycling between viewports) doesn't
    // re-drop and re-allocate per-tile TinyVecs every frame.
    let mut g = super::TextRectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    let big = g.tiles.len();
    g.start_frame(UVec2::new(256, 256));
    assert_eq!(g.tiles.len(), big, "shrink must not deallocate tiles");
}

// --- Occlusion-pruning tests -------------------------------------
//
// Pruning drops a quad iff a later quad in the same group fully
// covers its painted extent (`q.rect.inflated(stroke/2)`) under
// `Rect::contains_rect`. See `docs/roadmap/occlusion-pruning.md`.

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
            b.push_clip(rect(0.0, 0.0, 200.0, 200.0));
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
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    // A stroked quad's stroke spills outside the rect; pruning a
    // stroked quad on the strict containment test below would lose
    // the stroke fringe. Predicate requires zero-stroke as
    // occludable — stroked quads are kept regardless of cover.
    let buf = run(
        |b, _| {
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 0.0, 0.0).into()),
                Stroke::solid(Color::rgb(0.0, 1.0, 0.0), 2.0).into(),
            );
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
    // cover rect is its bounding rect deflated per side by
    // `max(adjacent_radii) * (1 - 1/sqrt(2))` ≈ 0.293·r. So a
    // rounded occluder strictly smaller (by the deflation margin)
    // than the under-quad does NOT fully cover it. Reversed: when
    // a sharp opaque quad on top exactly covers a rounded under,
    // the under is dropped (sharp cover == its own bounding rect,
    // which contains the rounded's bounding rect).
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    let buf_rounded_on_top = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // solid sharp under
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                crate::primitives::stroke::Stroke::ZERO.into(),
            );
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
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                crate::primitives::stroke::Stroke::ZERO.into(),
            );
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
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    // alpha=0.5 quad on top doesn't occlude anything beneath.
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0));
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgba(1.0, 1.0, 1.0, 0.5).into()),
                Stroke::ZERO.into(),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2, "semi-transparent does not occlude");
}

#[test]
fn prune_rounded_occluder_drops_smaller_under_inside_inscribed_rect() {
    // Phase 3: a rounded-corner opaque occluder fully covers a
    // sharp under-quad that fits entirely inside its inscribed
    // (KAPPA-deflated) rect. Rounded radius 10 ⇒ cover deflation
    // ≈ 2.93 per side → cover = (2.93, 2.93, 94.14, 94.14).
    // An under-quad at (10,10,80,80) is well inside cover and
    // should be dropped.
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 80.0, 80.0)); // sharp opaque under
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                crate::primitives::stroke::Stroke::ZERO.into(),
            );
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
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 5.0, 5.0)); // sharp under in corner
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(20.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                crate::primitives::stroke::Stroke::ZERO.into(),
            );
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
    use crate::primitives::brush::FillAxis;
    use crate::renderer::quad::FillKind;
    // A shadow's blur fringe extends past the stored rect — even if
    // a later opaque solid fully contains its rect, the visible
    // outer halo would be lost. Predicate must never drop shadows.
    let buf = run(
        |b, _| {
            b.draw_shadow(
                rect(20.0, 20.0, 60.0, 60.0),
                Corners::default(),
                Color::rgba(0.0, 0.0, 0.0, 0.5).into(),
                FillKind::SHADOW_DROP,
                // (offset.x, offset.y, sigma, spread) — sigma=4 ⇒ 8-px halo.
                FillAxis::from_lanes(0.0, 0.0, 4.0, 0.0),
            );
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
    // A solid-opaque occluder with a stroke still has its full rect
    // covered by the fill (the stroke just paints additional pixels
    // on/outside the edge — it doesn't subtract). A sharp under
    // entirely inside the occluder's rect should be dropped.
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::cmd_buffer::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 50.0, 50.0)); // sharp opaque under
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::solid(Color::rgb(0.0, 0.0, 0.0), 2.0).into(),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.quads.len(),
        1,
        "stroked occluder still covers its rect (fill alone is opaque)",
    );
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
    let mut buffer = RenderCmdBuffer::default();
    let mut composer = Composer::default();
    let display = params(1.0, UVec2::new(200, 200));
    for _ in 0..5 {
        buffer.clear();
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        let mut out = RenderBuffer::default();
        composer.compose(&buffer, &mut FrameArenaInner::default(), display, &mut out);
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

/// Regression: a quad overlapping text that lives in an *already-closed*
/// batch within the same group must still flush so the text paints under
/// it. Reproduces the node-label-over-inspector-panel bug: a node's text
/// gets closed into its own batch (here, by an unrelated polyline that
/// doesn't overlap), then the panel quad — recorded later, overlapping —
/// must not let that closed batch's text paint on top.
#[test]
fn quad_flushes_text_in_already_closed_batch_same_group() {
    let buf = run(
        |b, arena| {
            // Node label.
            text(b, rect(0.0, 0.0, 100.0, 20.0));
            // A polyline far from everything closes the text batch
            // (mesh-tier) without flushing the group, and doesn't overlap
            // the quad below (so it can't be what forces the flush).
            let p_start = arena.polyline_points.len() as u32;
            arena.polyline_points.push(Vec2::new(0.0, 400.0));
            arena.polyline_points.push(Vec2::new(50.0, 400.0));
            let c_start = arena.polyline_colors.len() as u32;
            arena.polyline_colors.push(Color::WHITE.into());
            b.draw_polyline(DrawPolylinePayload {
                bbox: rect(0.0, 400.0, 50.0, 0.0),
                origin: Vec2::ZERO,
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
            // Panel chrome quad, overlapping the (now closed-batch) label.
            draw(b, rect(0.0, 0.0, 100.0, 60.0));
        },
        &params(1.0, UVec2::new(600, 600)),
    );
    // The label's batch must anchor strictly before the group holding the
    // overlapping quad.
    let tlg = buf.text_batches[0].last_group;
    let qg = buf
        .groups
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            (g.quads.start..g.quads.start + g.quads.len)
                .any(|qi| buf.quads[qi as usize].rect.min.y < 100.0)
        })
        .map(|(i, _)| i as u32)
        .next()
        .expect("panel quad group");
    assert!(
        tlg < qg,
        "closed-batch text (last_group={tlg}) must paint before the overlapping quad (group={qg})",
    );
}
