//! The composer a test drives, the payloads it is fed, and what it is read
//! back through.

use crate::display::Display;
use crate::icons::icon_atlas::IconId;
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::primitives::interned_str::TextSource;
use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::primitives::{
    color::Color, color::ColorU8, corners::Corners, rect::Rect, stroke::Stroke,
};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::Composer;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    BrushSource, DrawIconPayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
    DrawQuadPayload, PushClipPayload,
};
use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
use crate::renderer::render_buffer::RenderBuffer;
use crate::scene::record_store::RecordPayloads;
use crate::scene::shapes::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use glam::{UVec2, Vec2};
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn composer() -> Composer {
    Composer::new(16_384)
}

pub(super) fn render_buffer() -> RenderBuffer {
    RenderBuffer::new()
}

pub(super) fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(x, y, w, h)
}

pub(super) fn clip(buf: &mut PaintCapture, r: Rect) {
    buf.clip(PushClipPayload::rect(r));
}

pub(super) fn clip_rounded(buf: &mut PaintCapture, r: Rect, corners: Corners) {
    buf.clip(PushClipPayload::rounded(r, corners));
}

pub(super) fn draw(buf: &mut PaintCapture, r: Rect) {
    buf.draw_quad(DrawQuadPayload::rect(
        r,
        Corners::default(),
        BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
        Stroke::ZERO.into(),
    ));
}

pub(super) fn text(buf: &mut PaintCapture, r: Rect) {
    buf.draw_text(
        r,
        Color::WHITE.into(),
        ShapedTextRef {
            key: TextShapeKey::INVALID,
            source: TextSource {
                span: Span::default(),
            },
        },
    );
}

pub(super) fn params(scale: f32, physical: UVec2) -> Display {
    Display {
        physical,
        scale_factor: scale,
        pixel_snap: false,
        refresh_millihertz: None,
    }
}

pub(super) fn run(
    build: impl FnOnce(&mut PaintCapture, &mut RecordPayloads),
    display: &Display,
) -> RenderBuffer {
    run_with_texture_cap(build, display, 16_384)
}

pub(super) fn run_with_texture_cap(
    build: impl FnOnce(&mut PaintCapture, &mut RecordPayloads),
    display: &Display,
    max_texture_dim: u32,
) -> RenderBuffer {
    let mut recorded = PaintCapture::default();
    let mut payloads = RecordPayloads::default();
    build(&mut recorded, &mut payloads);
    let mut composer = Composer::new(max_texture_dim);
    let mut out = render_buffer();
    composer
        .begin(*display, &payloads, &mut out)
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
    DrawImagePayload::image(rect, Vec2::ZERO, Vec2::ONE, Color::WHITE.into(), handle, 0)
}

/// The payload the encoder builds for an icon: a fit-resolved logical rect,
/// an identity, and a tint.
pub(super) fn icon(buf: &mut PaintCapture, r: Rect, icon: IconRef) {
    buf.draw_icon(DrawIconPayload {
        rect: r,
        icon,
        tint: Color::WHITE.into(),
        desaturate: false,
    });
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
    buf.draw_mesh(DrawMeshPayload {
        bbox,
        origin: Vec2::ZERO,
        tint: Color::WHITE.into(),
        v_start: 0,
        v_len: 3,
        i_start: 0,
        i_len: 3,
    });
}

pub(super) fn push_distinct_rounded_clips(buffer: &mut PaintCapture, depth: u32) {
    for level in 1..=depth {
        clip_rounded(
            buffer,
            rect(0.0, 0.0, 400.0, 400.0),
            Corners::all(level as f32),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn polyline_cmd(
    b: &mut PaintCapture,
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
        color_mode: mode,
        cap,
        join,
        ..Default::default()
    });
}

pub(super) fn curve(b: &mut PaintCapture, bbox: Rect) {
    use crate::renderer::frontend::payload::DrawCurvePayload;
    use crate::scene::shapes::paint::CurveBasis;
    b.draw_curve(DrawCurvePayload {
        bbox,
        origin: Vec2::ZERO,
        basis: CurveBasis::Cubic {
            p0: bbox.min,
            p1: Vec2::new(bbox.min.x + bbox.size.w * 0.3, bbox.max().y),
            p2: Vec2::new(bbox.min.x + bbox.size.w * 0.7, bbox.max().y),
            p3: bbox.max(),
        },
        color: Color::WHITE.into(),
        width: 2.0,
        ..Default::default()
    });
}

pub(super) fn image(b: &mut PaintCapture, r: Rect) {
    use crate::renderer::frontend::payload::DrawImagePayload;
    b.draw_image(
        DrawImagePayload::image(
            r,
            Vec2::ZERO,
            Vec2::ONE,
            Color::WHITE.into(),
            TextureId(1),
            0,
        ),
        None,
    );
}
