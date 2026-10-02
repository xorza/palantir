//! The composer a test drives, the payloads it is fed, and what it is read
//! back through.

use crate::display::Display;
use crate::gpu::gpu_frame_ctx::GpuFrameCtx;
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::IconId;
use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::primitives::{
    color::RgbaF32, color::rgba_f16::RgbaF16, corners::Corners, rect::Rect, stroke::Stroke,
};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::GpuPaint;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::RenderBuffer;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use glam::{UVec2, Vec2};
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

pub(super) fn composer() -> Composer {
    Composer::new(NonZeroU32::new(16_384).unwrap())
}

pub(super) fn render_buffer() -> RenderBuffer {
    RenderBuffer::new()
}

pub(super) fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

pub(super) fn clip(buf: &mut PaintCapture, r: Rect) {
    buf.push_clip(PushClipPayload::rect(r));
}

pub(super) fn clip_rounded(buf: &mut PaintCapture, r: Rect, corners: Corners) {
    buf.push_clip(PushClipPayload { rect: r, corners });
}

pub(super) fn draw(buf: &mut PaintCapture, r: Rect) {
    buf.draw_quad(
        DrawQuadPayload::rect(
            r,
            Corners::default(),
            BrushSource::Solid(RgbaF32::srgb(1.0, 1.0, 1.0).into()),
            Stroke::ZERO.into(),
        ),
        1.0,
    );
}

/// [`draw`] with a fill unique to the quad's place in the capture — the
/// n-th call's red channel is `n / 255` — so a test can name which quads
/// survived a prune, not only how many.
pub(super) fn draw_marked(buf: &mut PaintCapture, r: Rect) {
    let nth = buf.calls.len() as f32;
    buf.draw_quad(
        DrawQuadPayload::rect(
            r,
            Corners::default(),
            BrushSource::Solid(RgbaF32::new(nth / 255.0, 1.0, 1.0, 1.0).into()),
            Stroke::ZERO.into(),
        ),
        1.0,
    );
}

/// The surviving quads' rects, in buffer order.
pub(super) fn survivors(buf: &RenderBuffer) -> Vec<Rect> {
    buf.quads.iter().map(|q| q.rect).collect()
}

/// The surviving quads' capture positions, read back off the fill
/// [`draw_marked`] gave each: what tells apart two survivors over one rect.
pub(super) fn survivor_calls(buf: &RenderBuffer) -> Vec<u32> {
    buf.quads
        .iter()
        .map(|q| (q.fill.unpack().r * 255.0).round() as u32)
        .collect()
}

pub(super) fn text(buf: &mut PaintCapture, r: Rect) {
    buf.draw_text(
        DrawTextPayload {
            rect: r,
            color: RgbaF32::WHITE.into(),
            text: ShapedTextRef {
                key: TextShapeKey::fixture(),
                span: Span::default(),
            },
        },
        1.0,
    );
}

/// The display production composes under: pixel snap on.
pub(super) fn params(scale: f32, physical: UVec2) -> Display {
    Display::from_physical(physical, scale)
}

/// [`params`] with the pixel snap off — for a case about fractional
/// physical geometry, which the snap would round away before it reached
/// the code under test.
pub(super) fn params_unsnapped(scale: f32, physical: UVec2) -> Display {
    Display {
        pixel_snap: false,
        ..params(scale, physical)
    }
}

pub(super) fn run(
    build: impl FnOnce(&mut PaintCapture, &mut RecordStore),
    display: &Display,
) -> RenderBuffer {
    run_with_texture_cap(build, display, 16_384)
}

pub(super) fn run_with_texture_cap(
    build: impl FnOnce(&mut PaintCapture, &mut RecordStore),
    display: &Display,
    max_texture_dim: u32,
) -> RenderBuffer {
    let mut recorded = PaintCapture::default();
    let mut store = RecordStore::default();
    build(&mut recorded, &mut store);
    let mut composer = Composer::new(NonZeroU32::new(max_texture_dim).unwrap());
    let mut out = render_buffer();
    composer
        .begin(*display, Duration::ZERO, &store, &mut out)
        .replay_from(&recorded);
    out
}

#[derive(Debug)]
struct NoopGpuPaint;

impl GpuPaint for NoopGpuPaint {
    fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
}

pub(super) fn gpu_paint() -> GpuPaintRef {
    GpuPaintRef(Rc::new(RefCell::new(NoopGpuPaint)))
}

/// The payload the encoder builds for a `GpuView`: the view's full
/// arranged rect, untinted, full UV, default sampling. Pair it with
/// `Some(&paint)` — that's what makes the sink flag it as a view.
pub(super) fn gpu_view_payload(rect: Rect, handle: TextureId) -> DrawImagePayload {
    DrawImagePayload {
        rect,
        uv_min: Vec2::ZERO,
        uv_size: Vec2::ONE,
        tint: RgbaF32::WHITE.into(),
        handle,
        flags: 0,
    }
}

/// The payload the encoder builds for an icon: a fit-resolved logical rect,
/// an identity, and a tint.
pub(super) fn icon(buf: &mut PaintCapture, r: Rect, icon: IconRef) {
    buf.draw_icon(
        DrawIconPayload {
            rect: r,
            icon,
            tint: RgbaF32::WHITE.into(),
            desaturate: false,
        },
        1.0,
    );
}

pub(super) fn icon_ref(id: u16) -> IconRef {
    IconRef {
        set: IconSetId::new(0, 0),
        icon: IconId(id),
    }
}

pub(super) fn mesh(buf: &mut PaintCapture, bbox: Rect) {
    // 3 verts / 3 indices + opaque tint clears `DrawMeshPayload::is_noop`
    // so the cmd reaches the composer.
    buf.draw_mesh(
        DrawMeshPayload {
            bbox,
            origin: Vec2::ZERO,
            tint: RgbaF32::WHITE.into(),
            v_start: 0,
            v_len: 3,
            i_start: 0,
            i_len: 3,
        },
        1.0,
    );
}

pub(super) fn push_distinct_rounded_clips(buffer: &mut PaintCapture, depth: u32) {
    // Half-pixel steps keep every radius distinct and below the 200 px a
    // 400 px box fits, so no two levels fit to the same mask.
    for level in 1..=depth {
        clip_rounded(
            buffer,
            rect(0.0, 0.0, 400.0, 400.0),
            Corners::all(level as f32 * 0.5),
        );
    }
}

// Fixture builder: it mirrors the polyline command's own parameter list.
#[allow(clippy::too_many_arguments)]
pub(super) fn polyline_cmd(
    b: &mut PaintCapture,
    store: &mut RecordStore,
    points: &[Vec2],
    colors: &[RgbaF32],
    mode: ColorMode,
    width: f32,
    cap: LineCap,
    join: LineJoin,
) {
    let p_start = store.polyline_points.len() as u32;
    store.polyline_points.extend_from_slice(points);
    let c_start = store.polyline_colors.len() as u32;
    store
        .polyline_colors
        .extend(colors.iter().map(|&c| RgbaF16::from(c)));
    let mut lo = points[0];
    let mut hi = points[0];
    for &p in points {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    b.draw_polyline(
        DrawPolylinePayload {
            alpha: 1.0,
            bounds: StrokeBounds::Still(Rect::from_min_max(lo, hi)),
            origin: Vec2::ZERO,
            width,
            points_start: p_start,
            points_len: points.len() as u32,
            colors_start: c_start,
            colors_len: colors.len() as u32,
            color_mode: mode,
            cap,
            join,
        },
        1.0,
    );
}

pub(super) fn curve(b: &mut PaintCapture, bbox: Rect) {
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::scene::shapes::paint::curve_basis::CurveBasis;
    b.draw_curve(
        DrawCurvePayload {
            bounds: StrokeBounds::Still(bbox),
            origin: Vec2::ZERO,
            basis: CurveBasis::Cubic {
                p0: bbox.min,
                p1: Vec2::new(bbox.min.x + bbox.size.w * 0.3, bbox.max().y),
                p2: Vec2::new(bbox.min.x + bbox.size.w * 0.7, bbox.max().y),
                p3: bbox.max(),
            },
            fill: GpuFill {
                color: RgbaF32::WHITE.into(),
                ..Default::default()
            },
            width: 2.0,
            ..Default::default()
        },
        1.0,
    );
}

pub(super) fn image(b: &mut PaintCapture, r: Rect) {
    use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
    b.draw_image(
        ImageDraw {
            payload: DrawImagePayload {
                rect: r,
                uv_min: Vec2::ZERO,
                uv_size: Vec2::ONE,
                tint: RgbaF32::WHITE.into(),
                handle: TextureId(1),
                flags: 0,
            },
            view: None,
        },
        1.0,
    );
}
