use crate::display::Display;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::{FillKind, LutRow};
use crate::primitives::interned_str::TextSource;
use crate::primitives::span::Span;
use crate::primitives::{
    color::Color, color::ColorU8, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
    transform::TranslateScale, urect::URect,
};
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::cmd_buffer::payload::{
    BrushSource, ColorModeBits, DrawMeshPayload, DrawPolylinePayload, LineCapBits, LineJoinBits,
    ResolvedGradient,
};
use crate::renderer::frontend::composer::{Composer, stroke_bbox_scissor};
use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::texture_id::TextureId;
use crate::scene::record_store::RecordPayloads;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::TextShapeKey;
use glam::{UVec2, Vec2};
use std::cell::RefCell;
use std::f32::consts::FRAC_PI_2;
use std::rc::Rc;

fn composer() -> Composer {
    Composer::new(16_384)
}

fn render_buffer() -> RenderBuffer {
    RenderBuffer::new()
}

#[test]
#[should_panic(expected = "composer texture dimension limit must be positive")]
fn composer_rejects_zero_texture_limit() {
    let _ = Composer::new(0);
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

fn draw(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_rect(
        r,
        Corners::default(),
        BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
        Stroke::ZERO.into(),
    );
}

fn text(buf: &mut RenderCmdBuffer, r: Rect) {
    buf.draw_text(
        r,
        Color::WHITE.into(),
        TextShapeKey::INVALID,
        TextSource {
            span: Span::default(),
        },
    );
}

fn params(scale: f32, physical: UVec2) -> Display {
    Display {
        physical,
        scale_factor: scale,
        pixel_snap: false,
        refresh_millihertz: None,
    }
}

fn run(
    build: impl FnOnce(&mut RenderCmdBuffer, &mut RecordPayloads),
    display: &Display,
) -> RenderBuffer {
    run_with_texture_cap(build, display, 16_384)
}

fn run_with_texture_cap(
    build: impl FnOnce(&mut RenderCmdBuffer, &mut RecordPayloads),
    display: &Display,
    max_texture_dim: u32,
) -> RenderBuffer {
    let mut buffer = RenderCmdBuffer::default();
    let mut payloads = RecordPayloads::default();
    build(&mut buffer, &mut payloads);
    let mut composer = Composer::new(max_texture_dim);
    let mut out = render_buffer();
    composer.compose(&buffer, &payloads, *display, &mut out);
    out
}

#[derive(Debug)]
struct NoopGpuPaint;

impl GpuPaint for NoopGpuPaint {
    fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
}

fn gpu_paint() -> GpuPaintRef {
    GpuPaintRef(Rc::new(RefCell::new(NoopGpuPaint)))
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
fn stroke_bbox_scissor_applies_transform_dpi_and_style_once() {
    #[derive(Debug)]
    struct Case {
        scale: f32,
        cap: LineCap,
        join: Option<LineJoin>,
        expected: URect,
    }

    // Centerline (10,20)..(30,30), plus origin (2,4), then
    // x ↦ 1.5x + (3,5) gives logical (21,41)..(51,56).
    // Butt cases use physical pad = width_phys/2 + 0.5:
    // 0.5× → 2, 1× → 3.5, 2× → 6.5.
    let cases = [
        Case {
            scale: 0.5,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(8, 18, 20, 12),
        },
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(17, 37, 38, 23),
        },
        Case {
            scale: 2.0,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(35, 75, 74, 44),
        },
        // At 1×, Square pad = 3.5√2 ≈ 4.9498.
        Case {
            scale: 1.0,
            cap: LineCap::Square,
            join: None,
            expected: URect::new(16, 36, 40, 25),
        },
        // At 1×, Miter pad = 3.5·4 = 14.
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            join: Some(LineJoin::Miter),
            expected: URect::new(7, 27, 58, 43),
        },
    ];
    let xform = TranslateScale::new(Vec2::new(3.0, 5.0), 1.5);

    for case in cases {
        let actual = stroke_bbox_scissor(
            xform,
            rect(10.0, 20.0, 20.0, 10.0),
            Vec2::new(2.0, 4.0),
            4.0 * 1.5 * case.scale,
            case.cap,
            case.join,
            params(case.scale, UVec2::new(200, 200)),
        );
        assert_eq!(actual, case.expected, "{case:?}");
    }
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
            b.push_clip_rounded(rect(10.0, 10.0, 200.0, 200.0), Corners::all(8.0));
            draw(b, rect(20.0, 20.0, 40.0, 40.0));
            b.push_clip_rounded(rect(20.0, 20.0, 100.0, 100.0), Corners::all(4.0));
            draw(b, rect(30.0, 30.0, 20.0, 20.0));
            b.push_clip(rect(30.0, 30.0, 50.0, 50.0));
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

fn push_distinct_rounded_clips(buffer: &mut RenderCmdBuffer, depth: u32) {
    for level in 1..=depth {
        buffer.push_clip_rounded(rect(0.0, 0.0, 400.0, 400.0), Corners::all(level as f32));
    }
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
            b.push_clip_rounded(rect(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
            draw(b, rect(20.0, 20.0, 20.0, 20.0));
            b.push_clip_rounded(rect(10.0, 10.0, 100.0, 100.0), Corners::all(8.0));
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
            b.push_clip_rounded(rect(-50.0, -20.0, 200.0, 100.0), Corners::all(8.0));
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
            b.push_clip(rect(10.0, 20.0, 100.0, 80.0));
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
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
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
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    let mut buffer = RenderCmdBuffer::default();
    buffer.draw_rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        BrushSource::Solid(Color::rgb(0.5, 0.5, 0.5).into()),
        Stroke::ZERO.into(),
    );
    let mut composer = composer();
    let mut out = render_buffer();
    // 200×200 viewport: an opaque solid sharp quad covering the whole
    // viewport would fold into the clear instead of emitting a quad.
    composer.compose(
        &buffer,
        &RecordPayloads::default(),
        params(1.0, UVec2::new(200, 200)),
        &mut out,
    );
    let q = &out.quads[0];
    assert_eq!(
        q.fill_kind,
        // Sharp + stroke-less + pixel-aligned, so the solid kind also
        // carries the fragment fast-path bit.
        FillKind::SOLID.with_fast(),
        "solid quad must carry kind=solid (+fast)",
    );
    assert_eq!(
        q.fill_lut_row,
        LutRow::FALLBACK,
        "solid quad has no LUT row",
    );
    assert_eq!(q.fill_axis, FillAxis::ZERO, "solid quad axis is zeroed",);
}

/// A windowed rect must never fold into the pass clear, take the
/// fragment fast path, or occlude quads beneath it — its interior is
/// a hole. All three opaque-cover optimizations compare
/// `fill_kind == FillKind::SOLID` exactly; the window bit breaks that
/// equality by design. Deliberate worst case: full-viewport, opaque,
/// solid, sharp-cornered, pixel-aligned at scale 1 — without the
/// window bit this exact draw would trigger all three.
#[test]
fn windowed_rect_is_not_an_opaque_cover() {
    use crate::primitives::fill_wire::FillKind;
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 50.0, 50.0));
            b.draw_rect_window(
                rect(0.0, 0.0, 200.0, 200.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(
        buf.clear_override.is_none(),
        "windowed cover must not clear-fold",
    );
    assert_eq!(
        buf.quads.len(),
        2,
        "under-quad survives beneath a windowed cover",
    );
    assert_eq!(
        buf.quads[1].fill_kind,
        FillKind::SOLID.with_window(),
        "window bit rides through to the Quad; fast bit absent",
    );
}

/// A resolved linear gradient packs row + axis + kind into the
/// cmd-buffer payload; composer pipes them through to the emitted Quad.
#[test]
fn compose_linear_brush_emits_kind_one_with_atlas_row() {
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::brush::gradient::linear::LinearGradient;
    use crate::primitives::fill_wire::FillKind;
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    let g =
        LinearGradient::two_stop(0.0, ColorU8::WHITE, ColorU8::BLACK).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let atlas = SharedGradientAtlas::default();
    let row = atlas.register_stops(&g.stops, g.interp);
    let lowered = ResolvedGradient {
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
    let mut composer = composer();
    let mut out = render_buffer();
    composer.compose(
        &buffer,
        &RecordPayloads::default(),
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
    use crate::primitives::brush::gradient::linear::LinearGradient;
    use crate::primitives::fill_wire::FillKind;
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    let g = LinearGradient::two_stop(0.5, ColorU8::hex(0x336699), ColorU8::hex(0xddaa44));
    let atlas = SharedGradientAtlas::default();
    let lowered = ResolvedGradient {
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
    let mut composer = composer();
    let mut out = render_buffer();
    composer.compose(
        &buffer,
        &RecordPayloads::default(),
        params(1.0, UVec2::new(100, 100)),
        &mut out,
    );
    let rows: Vec<_> = out.quads.iter().map(|q| q.fill_lut_row).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    assert!(rows[0].0 >= 1);
}

/// Pin: text-run scale snaps to the additive 0.5% ladder so continuous
/// zoom produces stable glyphon cache keys across adjacent frames.
/// Quads (next test) intentionally do not snap — only text quantizes.
#[test]
fn compose_snaps_text_scale_to_discrete_steps() {
    // 1.013 is between 1.010 and 1.015; rounds to 1.015.
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
        (s - 1.015).abs() < 1e-5,
        "1.013 must snap to 1.015, got {s}",
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
            b.push_transform(TranslateScale::new(Vec2::new(3.0, 5.0), 2.0));
            b.push_transform(TranslateScale::new(Vec2::new(7.0, 11.0), 4.0));
            draw(b, rect(-2.0, 3.0, 4.0, 5.0));
            b.pop_transform();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(1.0, 51.0));
    assert_eq!(q.rect.size, Size::new(32.0, 40.0));
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
            b.draw_shadow(
                shadow_rect,
                Corners::ZERO,
                Color::BLACK.into(),
                FillKind::SHADOW_DROP,
                FillAxis::from_lanes(0.0, 0.0, sigma, 0.0),
            );
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
/// clips → different stencil masks at render time; one merged prepare
/// would mis-clip text under one of them. Each batch also carries the
/// mask chain its runs were recorded under, value-matching its
/// `last_group`'s chain — the schedule needs it to stencil a batch
/// drained past damage-skipped groups against the right mask.
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
    assert_eq!(buf.curve_batches.len(), 1);
    assert_eq!(
        buf.curve_batches[0].items.len, 1,
        "one segment instance for a 2-point polyline",
    );
    assert!(buf.meshes.is_empty(), "no CPU-tessellated mesh");
}

#[allow(clippy::too_many_arguments)]
fn polyline_cmd(
    b: &mut RenderCmdBuffer,
    payloads: &mut RecordPayloads,
    points: &[Vec2],
    colors: &[Color],
    mode: ColorMode,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) {
    let p_start = payloads.polyline_points.len() as u32;
    payloads.polyline_points.extend_from_slice(points);
    let c_start = payloads.polyline_colors.len() as u32;
    payloads
        .polyline_colors
        .extend(colors.iter().map(|&c| ColorU8::from(c)));
    let mut lo = points[0];
    let mut hi = points[0];
    for &p in points {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    b.draw_polyline(DrawPolylinePayload {
        bbox: Rect::from_min_max(lo, hi),
        origin: Vec2::ZERO,
        width,
        points_start: p_start,
        points_len: points.len() as u32,
        colors_start: c_start,
        colors_len: colors.len() as u32,
        color_mode: ColorModeBits::new(mode),
        cap: LineCapBits::new(cap),
        join: LineJoinBits::new(join),
        ..bytemuck::Zeroable::zeroed()
    });
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
    let mut commands = RenderCmdBuffer::default();
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
    composer.compose(
        &commands,
        &payloads,
        params(1.0, UVec2::new(200, 200)),
        &mut buf,
    );
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
        let mut buffer = RenderCmdBuffer::default();
        let mut payloads = RecordPayloads::default();
        let p_start = payloads.polyline_points.len() as u32;
        payloads.polyline_points.push(Vec2::new(15.0, 50.0));
        payloads.polyline_points.push(Vec2::new(85.0, 50.0));
        let c_start = payloads.polyline_colors.len() as u32;
        payloads.polyline_colors.push(Color::WHITE.into());
        buffer.draw_polyline(DrawPolylinePayload {
            bbox: rect(0.0, 0.0, 100.0, 100.0),
            origin: Vec2::ZERO,
            width: 2.0,
            rotation,
            points_start: p_start,
            points_len: 2,
            colors_start: c_start,
            colors_len: 1,
            color_mode: ColorModeBits::new(ColorMode::Single),
            cap: LineCapBits::new(LineCap::Butt),
            join: LineJoinBits::new(LineJoin::Miter),
            ..bytemuck::Zeroable::zeroed()
        });
        let mut composer = composer();
        let mut out = render_buffer();
        composer.compose(
            &buffer,
            &payloads,
            params(1.0, UVec2::new(200, 200)),
            &mut out,
        );
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
            b.push_clip(rect(0.0, 0.0, 100.0, 100.0));
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
    assert!(buf.mesh_batches.is_empty());
    assert!(buf.image_batches.is_empty());
    assert!(buf.curve_batches.is_empty());
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
    use crate::renderer::frontend::cmd_buffer::payload::DrawImagePayload;
    let buf = run(
        |b, _arena| {
            b.draw_image(DrawImagePayload::image(
                rect(10.0, 20.0, 30.0, 40.0),
                glam::Vec2::ZERO,
                glam::Vec2::ONE,
                Color::WHITE.into(),
                TextureId(0xc0ffee),
                0,
            ));
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.len(), 1, "one image draw");
    assert_eq!(buf.images.len(), 1, "one image instance");
    assert_eq!(buf.image_batches.len(), 1, "one image batch");
    assert_eq!(buf.image_batches[0].items, Span::new(0, 1));
    assert_eq!(buf.images.id()[0], TextureId(0xc0ffee));
    // Physical-px rect = logical * scale (no snap in `params`).
    assert_eq!(buf.images.instance()[0].rect, rect(20.0, 40.0, 60.0, 80.0));
    // Composer must forward the encoder's UV crop verbatim — a Zero
    // UV size means "sample one texel forever" and silently paints
    // every image as a uniform color (regression hunt: 2026-05).
    assert_eq!(buf.images.instance()[0].uv_min, glam::Vec2::ZERO);
    assert_eq!(buf.images.instance()[0].uv_size, glam::Vec2::ONE);
}

#[test]
fn compose_gpu_view_carries_nested_transform_and_dpr_to_raster_target() {
    #[derive(Debug)]
    struct Case {
        dpr: f32,
        expected_size: UVec2,
        expected_raster_scale: f32,
    }

    let cases = [
        Case {
            dpr: 1.0,
            expected_size: UVec2::new(60, 30),
            expected_raster_scale: 3.0,
        },
        Case {
            dpr: 2.0,
            expected_size: UVec2::new(120, 60),
            expected_raster_scale: 6.0,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _arena| {
                b.push_transform(TranslateScale::from_scale(2.0));
                b.push_transform(TranslateScale::from_scale(1.5));
                b.draw_gpu_view(rect(0.0, 0.0, 20.0, 10.0), TextureId(0xc0ffee), gpu_paint());
                b.pop_transform();
                b.pop_transform();
            },
            &params(case.dpr, UVec2::new(512, 512)),
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_size, "{case:?}");
        assert_eq!(target.display_scale, case.dpr, "{case:?}");
        assert_eq!(target.raster_scale, case.expected_raster_scale, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].rect.size,
            Size::new(case.expected_size.x as f32, case.expected_size.y as f32),
            "{case:?}"
        );
    }
}

#[test]
fn compose_gpu_view_caps_wide_and_tall_targets_uniformly() {
    #[derive(Debug)]
    struct Case {
        logical_size: Size,
        expected_target: UVec2,
    }

    let cases = [
        Case {
            logical_size: Size::new(200.0, 50.0),
            expected_target: UVec2::new(100, 25),
        },
        Case {
            logical_size: Size::new(50.0, 200.0),
            expected_target: UVec2::new(25, 100),
        },
    ];

    for case in cases {
        let buf = run_with_texture_cap(
            |b, _arena| {
                b.draw_gpu_view(
                    Rect {
                        min: Vec2::ZERO,
                        size: case.logical_size,
                    },
                    TextureId(0xc0ffee),
                    gpu_paint(),
                );
            },
            &params(1.0, UVec2::new(400, 400)),
            100,
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_target, "{case:?}");
        assert_eq!(target.display_scale, 1.0, "{case:?}");
        assert_eq!(target.raster_scale, 0.5, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].rect.size,
            case.logical_size,
            "the composite destination stays at monitor resolution: {case:?}"
        );
        assert_eq!(
            target.used.x as f32 * case.logical_size.h,
            target.used.y as f32 * case.logical_size.w,
            "the capped target preserves the composite aspect ratio: {case:?}"
        );
    }
}

#[test]
fn compose_image_forwards_uv_crop_for_cover_fit() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawImagePayload;
    let buf = run(
        |b, _arena| {
            b.draw_image(DrawImagePayload::image(
                rect(0.0, 0.0, 100.0, 100.0),
                glam::Vec2::new(0.25, 0.0),
                glam::Vec2::new(0.5, 1.0),
                Color::WHITE.into(),
                TextureId(1),
                0,
            ));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].uv_min, glam::Vec2::new(0.25, 0.0));
    assert_eq!(buf.images.instance()[0].uv_size, glam::Vec2::new(0.5, 1.0));
}

/// The composer forwards `flags` verbatim and keeps each draw's UV as-is
/// (a `GpuView` ships full UV from the encoder — see `gpu_view` tests).
#[test]
fn compose_forwards_flags_and_repeat_uv() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawImagePayload;
    use crate::renderer::render_buffer::image::{
        IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TILED,
    };
    let buf = run(
        |b, _arena| {
            // Plain draw: flags stay 0.
            b.draw_image(DrawImagePayload::image(
                rect(0.0, 0.0, 50.0, 50.0),
                glam::Vec2::ZERO,
                glam::Vec2::ONE,
                Color::WHITE.into(),
                TextureId(1),
                0,
            ));
            // Tiled draw: UV size > 1 (3×2 repeats) + tiled bit.
            b.draw_image(DrawImagePayload::image(
                rect(0.0, 0.0, 50.0, 50.0),
                glam::Vec2::ZERO,
                glam::Vec2::new(3.0, 2.0),
                Color::WHITE.into(),
                TextureId(2),
                IMG_FLAG_TILED,
            ));
            // The two nearest-filter bits ride through together.
            b.draw_image(DrawImagePayload::image(
                rect(0.0, 0.0, 50.0, 50.0),
                glam::Vec2::ZERO,
                glam::Vec2::ONE,
                Color::WHITE.into(),
                TextureId(3),
                IMG_FLAG_MIN_NEAREST | IMG_FLAG_MAG_NEAREST,
            ));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].flags, 0);
    assert_eq!(buf.images.instance()[1].flags, IMG_FLAG_TILED);
    assert_eq!(buf.images.instance()[1].uv_size, glam::Vec2::new(3.0, 2.0));
    assert_eq!(
        buf.images.instance()[2].flags,
        IMG_FLAG_MIN_NEAREST | IMG_FLAG_MAG_NEAREST
    );
}

#[test]
fn compose_emits_one_curve_batch_per_scissor_group() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
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
    assert!(batch.items.len >= 2 && batch.items.len.is_multiple_of(2));
    assert_eq!(
        buf.curves.len() as u32,
        batch.items.len,
        "batch covers every emitted instance",
    );
}

#[test]
fn compose_splits_curve_batches_across_scissor_groups() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
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
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::fill_wire::FillKind;
    use crate::primitives::fill_wire::LutRow;
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
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

#[test]
fn compose_arc_scales_geometry_and_subdivides_by_exact_length() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawArcPayload;
    use crate::renderer::render_buffer::curve::CURVE_KIND_ARC;
    use std::f32::consts::PI;
    // 3/4 arc: r = 20 logical, sweep = 1.5π, at DPI scale 2.
    let sweep = 1.5 * PI;
    let buf = run(
        |b, _arena| {
            b.draw_arc(DrawArcPayload {
                bbox: rect(0.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                center: Vec2::new(50.0, 50.0),
                radius: 20.0,
                a0: 0.0,
                a1: sweep,
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
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
    assert_eq!(buf.curve_batches.len(), 1);
    assert_eq!(buf.curve_batches[0].items.len, 8);
}

#[test]
fn compose_arc_spin_rotates_center_about_bbox_pivot_and_offsets_angles() {
    use crate::renderer::frontend::cmd_buffer::payload::DrawArcPayload;
    use std::f32::consts::{FRAC_PI_2, PI};
    // Pivot = bbox.center() = (50, 50); center (70, 50) is +20 along x.
    // rotation = π/2 (clockwise on screen, y-down): (+20, 0) → (0, +20),
    // so the spun center is (50, 70). Both angles shift by π/2.
    let buf = run(
        |b, _arena| {
            b.draw_arc(DrawArcPayload {
                bbox: rect(0.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                center: Vec2::new(70.0, 50.0),
                radius: 10.0,
                a0: 0.0,
                a1: PI,
                rotation: FRAC_PI_2,
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
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
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
    // Same 800 px span: a straight cubic (CPs on the segment thirds —
    // exactly what Shape::line lowers to) must collapse to one
    // instance; a genuinely curved one must subdivide (800 px polygon
    // → ⌈⌈800/1.5⌉/16⌉ = 34 instances).
    let straight = |b: &mut RenderCmdBuffer| {
        b.draw_curve(DrawCurvePayload {
            bbox: rect(0.0, 0.0, 800.0, 10.0),
            origin: Vec2::ZERO,
            p0: Vec2::new(0.0, 5.0),
            p1: Vec2::new(800.0 / 3.0, 5.0),
            p2: Vec2::new(1600.0 / 3.0, 5.0),
            p3: Vec2::new(800.0, 5.0),
            color: Color::WHITE.into(),
            width: 2.0,
            ..bytemuck::Zeroable::zeroed()
        });
    };
    let curved = |b: &mut RenderCmdBuffer| {
        b.draw_curve(DrawCurvePayload {
            bbox: rect(0.0, 0.0, 800.0, 400.0),
            origin: Vec2::ZERO,
            p0: Vec2::new(0.0, 5.0),
            p1: Vec2::new(266.0, 400.0),
            p2: Vec2::new(533.0, 400.0),
            p3: Vec2::new(800.0, 5.0),
            color: Color::WHITE.into(),
            width: 2.0,
            ..bytemuck::Zeroable::zeroed()
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
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
    use std::f32::consts::FRAC_PI_2;
    // Pivot = bbox.center() = (50, 50). A π/2 spin (clockwise on
    // screen, y-down) maps an offset (dx, dy) from the pivot to
    // (-dy, dx). p0 = (70, 50) → (50, 70); p3 = (50, 30) → (70, 50).
    let buf = run(
        |b, _arena| {
            b.draw_curve(DrawCurvePayload {
                bbox: rect(0.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                rotation: FRAC_PI_2,
                p0: Vec2::new(70.0, 50.0),
                p1: Vec2::new(70.0, 40.0),
                p2: Vec2::new(60.0, 30.0),
                p3: Vec2::new(50.0, 30.0),
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
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
    use crate::renderer::frontend::cmd_buffer::payload::{DrawArcPayload, DrawCurvePayload};
    use crate::renderer::render_buffer::curve::{CURVE_KIND_ARC, CURVE_KIND_CUBIC};
    let buf = run(
        |b, _arena| {
            b.draw_arc(DrawArcPayload {
                bbox: rect(0.0, 0.0, 40.0, 40.0),
                origin: Vec2::ZERO,
                center: Vec2::new(20.0, 20.0),
                radius: 10.0,
                a0: 0.0,
                a1: 1.0,
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
            });
            b.draw_curve(DrawCurvePayload {
                bbox: rect(100.0, 0.0, 100.0, 100.0),
                origin: Vec2::ZERO,
                p0: Vec2::new(100.0, 0.0),
                p1: Vec2::new(110.0, 50.0),
                p2: Vec2::new(190.0, 50.0),
                p3: Vec2::new(200.0, 0.0),
                color: Color::WHITE.into(),
                width: 2.0,
                ..bytemuck::Zeroable::zeroed()
            });
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(buf.curve_batches.len(), 1, "arcs batch with cubics");
    assert_eq!(buf.curve_batches[0].items.len as usize, buf.curves.len());
    assert!(buf.curves.iter().any(|c| c.kind == CURVE_KIND_ARC));
    assert!(buf.curves.iter().any(|c| c.kind == CURVE_KIND_CUBIC));
}

fn curve(b: &mut RenderCmdBuffer, bbox: Rect) {
    use crate::renderer::frontend::cmd_buffer::payload::DrawCurvePayload;
    b.draw_curve(DrawCurvePayload {
        bbox,
        origin: Vec2::ZERO,
        p0: bbox.min,
        p1: Vec2::new(bbox.min.x + bbox.size.w * 0.3, bbox.max().y),
        p2: Vec2::new(bbox.min.x + bbox.size.w * 0.7, bbox.max().y),
        p3: bbox.max(),
        color: Color::WHITE.into(),
        width: 2.0,
        ..bytemuck::Zeroable::zeroed()
    });
}

fn image(b: &mut RenderCmdBuffer, r: Rect) {
    use crate::renderer::frontend::cmd_buffer::payload::DrawImagePayload;
    b.draw_image(DrawImagePayload::image(
        r,
        Vec2::ZERO,
        Vec2::ONE,
        Color::WHITE.into(),
        TextureId(1),
        0,
    ));
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
    assert_eq!(buf.curve_batches.len(), 1);
    assert_eq!(buf.curve_batches[0].last_group, 0);
    assert_eq!(buf.mesh_batches.len(), 1);
    assert_eq!(buf.mesh_batches[0].last_group, 1);
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
    assert_eq!(buf.mesh_batches[0].last_group, 0);
    assert_eq!(buf.curve_batches[0].last_group, 0);
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
    assert_eq!(buf.mesh_batches[0].last_group, 0);
    assert_eq!(buf.image_batches[0].last_group, 0);

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
    assert_eq!(buf.image_batches[0].last_group, 0);
    assert_eq!(buf.mesh_batches[0].last_group, 1);
}

#[test]
fn compose_image_curve_record_order_and_same_tier_gate_group_split() {
    let buf = run(
        |b, _| {
            image(b, rect(10.0, 10.0, 30.0, 30.0));
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "image then curve: replay == record");
    assert_eq!(buf.image_batches[0].last_group, 0);
    assert_eq!(buf.curve_batches[0].last_group, 0);

    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
            image(b, rect(10.0, 10.0, 30.0, 30.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.groups.len(),
        2,
        "curve then image: replay inverts record",
    );
    assert_eq!(buf.curve_batches[0].last_group, 0);
    assert_eq!(buf.image_batches[0].last_group, 1);

    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 50.0, 100.0, 0.0));
            curve(b, rect(0.0, 50.0, 100.0, 0.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "same-tier order is stable");
    assert_eq!(buf.curves.len(), 2);
    assert_eq!(buf.curve_batches.len(), 1);
    assert_eq!(buf.curve_batches[0].last_group, 0);
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
    assert_eq!(buf.curve_batches[0].last_group, 0);
    assert_eq!(buf.mesh_batches[0].last_group, 0);
    assert_eq!(buf.image_batches[0].last_group, 0);
}

//
// Pruning drops a quad iff a later quad in the same group fully
// covers its painted extent (`q.rect.inflated(stroke/2)`) under
// `Rect::contains_rect`.

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
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
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
    // cover rect is its bounding rect deflated per side by the corner
    // inset plus the SDF's 0.5px AA transition. So a rounded occluder
    // strictly smaller (by the deflation margin)
    // than the under-quad does NOT fully cover it. Reversed: when
    // a sharp opaque quad on top exactly covers a rounded under,
    // the under is dropped (sharp cover == its own bounding rect,
    // which contains the rounded's bounding rect).
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    let buf_rounded_on_top = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 100.0, 100.0)); // solid sharp under
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
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
                Stroke::ZERO.into(),
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
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
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
    // (KAPPA-deflated) rect. Rounded radius 10 plus the AA transition
    // gives a cover deflation of ≈3.43 per side.
    // An under-quad at (10,10,80,80) is well inside cover and
    // should be dropped.
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 80.0, 80.0)); // sharp opaque under
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(10.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
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
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
    let buf = run(
        |b, _| {
            draw(b, rect(0.0, 0.0, 5.0, 5.0)); // sharp under in corner
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::all(20.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
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
    use crate::primitives::brush::gradient::FillAxis;
    use crate::primitives::fill_wire::FillKind;
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
    // A solid-opaque occluder with a fully-OPAQUE stroke covers its
    // whole rect: quad.wgsl strokes are inner-edge and coverage-
    // partitioned with the fill, so opaque annulus + opaque fill =
    // opaque rect. A sharp under entirely inside should be dropped.
    // (Translucent strokes shrink the cover — see
    // `prune_occluder_stroke_translucency_gates_cover`.)
    use crate::primitives::stroke::Stroke;
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
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
    use crate::renderer::frontend::cmd_buffer::payload::BrushSource;
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
                b.draw_rect(
                    rect(0.0, 0.0, 100.0, 100.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    (&case.stroke).into(),
                );
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
    let mut buffer = RenderCmdBuffer::default();
    let mut composer = composer();
    let display = params(1.0, UVec2::new(200, 200));
    for _ in 0..5 {
        buffer.clear();
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        draw(&mut buffer, rect(0.0, 0.0, 100.0, 100.0));
        let mut out = render_buffer();
        composer.compose(&buffer, &RecordPayloads::default(), display, &mut out);
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
        |b, payloads| {
            // Node label.
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
    use crate::primitives::fill_wire::{FillKind, LutRow};

    let vp = UVec2::new(200, 200);
    let bg = Color::rgb(0.14, 0.16, 0.22);
    // The override rides a ColorF16 lane; expected value is the f16
    // round-trip of the input, not the input itself.
    let folded = ColorF16::from(bg).unpack();

    // (case, builder, expected quad count, expected override)
    type Build = fn(&mut RenderCmdBuffer);
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
                b.draw_rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::all(4.0),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    Stroke::ZERO.into(),
                );
            },
            1,
            None,
        ),
        (
            "stroke disqualifies",
            |b| {
                b.draw_rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                    Stroke::solid(Color::WHITE, 2.0).into(),
                );
            },
            1,
            None,
        ),
        (
            "translucent fill disqualifies",
            |b| {
                b.draw_rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgba(1.0, 1.0, 1.0, 0.5).into()),
                    Stroke::ZERO.into(),
                );
            },
            1,
            None,
        ),
        (
            "gradient fill disqualifies",
            |b| {
                b.draw_rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Gradient(ResolvedGradient {
                        axis: FillAxis::ZERO,
                        row: LutRow::FALLBACK,
                        kind: FillKind::linear(Spread::Pad),
                    }),
                    Stroke::ZERO.into(),
                );
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
                b.push_clip(rect(0.0, 0.0, 150.0, 150.0));
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
                b.draw_rect(
                    rect(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(Color::rgb(0.14, 0.16, 0.22).into()),
                    Stroke::ZERO.into(),
                );
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
            b.draw_rect(
                rect(0.0, 0.0, 100.0, 100.0),
                Corners::default(),
                BrushSource::Solid(bg.into()),
                Stroke::ZERO.into(),
            );
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
            b.push_clip(rect(0.0, 0.0, 150.0, 150.0));
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

    let mut covered = RenderCmdBuffer::default();
    draw(&mut covered, rect(0.0, 0.0, 200.0, 200.0));
    draw(&mut covered, rect(10.0, 10.0, 20.0, 20.0));

    composer.compose(&covered, &payloads, display, &mut out);
    assert!(out.clear_override.is_some(), "frame 1 folds");
    assert_eq!(out.quads.len(), 1);

    composer.compose(&covered, &payloads, display, &mut out);
    assert!(out.clear_override.is_some(), "steady state re-folds");
    assert_eq!(out.quads.len(), 1);

    let mut uncovered = RenderCmdBuffer::default();
    draw(&mut uncovered, rect(10.0, 10.0, 20.0, 20.0));
    composer.compose(&uncovered, &payloads, display, &mut out);
    assert_eq!(out.clear_override, None, "no cover, no override");
    assert_eq!(out.quads.len(), 1);
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
            |b, _arena| b.draw_rect(*r, *corners, *brush, (*stroke).into()),
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
