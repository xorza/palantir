//! The composer a test drives, the payloads it is fed, and how it is read back.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and color channels"
)]

use crate::common::span::Span;
use crate::display::Display;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::icon_ref::IconRef;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::composer::tests::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::shape::record::ColorMode;
use crate::shape::record_store::RecordStore;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

pub(super) fn clip(buf: &mut PaintCapture, r: Rect) {
    buf.push_clip(PushClipPayload::rect(r));
}

pub(super) fn clip_rounded(buf: &mut PaintCapture, r: Rect, corners: Corners) {
    buf.push_clip(PushClipPayload { rect: r, corners });
}

pub(super) fn draw(buf: &mut PaintCapture, r: Rect) {
    QuadBuilder::new(r).draw(buf);
}

/// [`draw`] with a fill unique to the quad's capture position (n-th call: red `n / 255`), so a test can name which quads survived a prune.
pub(super) fn draw_marked(buf: &mut PaintCapture, r: Rect) {
    let nth = buf.calls.len() as f32;
    QuadBuilder::new(r)
        .solid(RgbaF32::new(nth / 255.0, 1.0, 1.0, 1.0))
        .draw(buf);
}

/// The surviving quads' rects, in buffer order.
pub(super) fn survivors(buf: &RenderBuffer) -> Vec<Rect> {
    buf.quads.iter().map(|q| q.rect).collect()
}

/// Surviving capture positions, read off the fill [`draw_marked`] gave; tells apart survivors over one rect.
pub(super) fn survivor_calls(buf: &RenderBuffer) -> Vec<u32> {
    buf.quads
        .iter()
        .map(|q| (q.fill.unpack().r * 255.0).round() as u32)
        .collect()
}

pub(super) fn text(buf: &mut PaintCapture, r: Rect) {
    inked_text(buf, r, Spacing::ZERO);
}

/// A [`text`] run whose glyphs' ink reaches `ink` past `r`.
pub(super) fn inked_text(buf: &mut PaintCapture, r: Rect, ink: Spacing) {
    buf.draw_text(
        DrawTextPayload {
            rect: r,
            ink,
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

/// [`params`] with pixel snap off, for fractional physical geometry the snap would round away.
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
    run_in(ComposeRig::new(*display), build)
}

pub(super) fn run_with_texture_cap(
    build: impl FnOnce(&mut PaintCapture, &mut RecordStore),
    display: &Display,
    max_texture_dim: u32,
) -> RenderBuffer {
    let cap = NonZeroU32::new(max_texture_dim).unwrap();
    run_in(ComposeRig::with_texture_cap(*display, cap), build)
}

/// One frame of `rig`, recorded by `build` into a fresh capture and the rig's store.
fn run_in(
    mut rig: ComposeRig,
    build: impl FnOnce(&mut PaintCapture, &mut RecordStore),
) -> RenderBuffer {
    let mut recorded = PaintCapture::default();
    build(&mut recorded, &mut rig.store);
    rig.compose(&recorded);
    rig.out
}

/// The payload the encoder builds for a `GpuView`: full arranged rect, untinted, full UV, default sampling. Pair with `Some(&paint)` so the sink flags it a view.
pub(super) fn gpu_view_payload(rect: Rect, handle: TextureId) -> DrawImagePayload {
    DrawImagePayload {
        rect,
        uv_min: Vec2::ZERO,
        uv_size: Vec2::ONE,
        tint: RgbaF32::WHITE.into(),
        handle,
        flags: ImageFlags::NONE,
    }
}

/// Draw a `GpuView` at `rect`: [`gpu_view_payload`] under a no-op paint, full alpha.
pub(super) fn gpu_view(buf: &mut PaintCapture, rect: Rect, handle: TextureId) {
    buf.draw_image(
        ImageDraw {
            payload: gpu_view_payload(rect, handle),
            view: Some(ViewPaint {
                paint: &GpuPaintRef::noop(),
                epoch: 0,
            }),
        },
        1.0,
    );
}

/// The payload the encoder builds for an icon: fit-resolved logical rect, identity, tint.
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

pub(super) fn mesh(buf: &mut PaintCapture, bbox: Rect) {
    // 3 verts / 3 indices and an opaque tint clear `DrawMeshPayload::is_noop`.
    buf.draw_mesh(
        DrawMeshPayload {
            bbox,
            origin: Vec2::ZERO,
            tint: RgbaF32::WHITE.into(),
            vertices: Span::new(0, 3),
            indices: Span::new(0, 3),
        },
        1.0,
    );
}

pub(super) fn push_distinct_rounded_clips(buffer: &mut PaintCapture, depth: u32) {
    // Half-pixel steps keep every radius distinct and under the 200 px a 400 px box fits.
    for level in 1..=depth {
        clip_rounded(
            buffer,
            Rect::new(0.0, 0.0, 400.0, 400.0),
            Corners::all(level as f32 * 0.5),
        );
    }
}

/// A polyline command a composer test records: white, one color, butt-capped
/// and mitred unless set.
#[derive(Clone, Copy, Debug)]
pub(super) struct TestPolyline<'a> {
    points: &'a [Vec2],
    width: f32,
    colors: &'a [RgbaF32],
    mode: ColorMode,
    cap: LineCap,
    join: LineJoin,
}

impl<'a> TestPolyline<'a> {
    pub(super) const fn new(points: &'a [Vec2], width: f32) -> Self {
        Self {
            points,
            width,
            colors: &[RgbaF32::WHITE],
            mode: ColorMode::Single,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
        }
    }

    pub(super) const fn colors(self, colors: &'a [RgbaF32], mode: ColorMode) -> Self {
        Self {
            colors,
            mode,
            ..self
        }
    }

    pub(super) const fn cap(self, cap: LineCap) -> Self {
        Self { cap, ..self }
    }

    pub(super) const fn join(self, join: LineJoin) -> Self {
        Self { join, ..self }
    }

    /// Lowers the points and colors into `store` and records the draw onto `b`.
    pub(super) fn record(self, b: &mut PaintCapture, store: &mut RecordStore) {
        let Self {
            points,
            width,
            colors,
            mode,
            cap,
            join,
        } = self;
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
                points: Span::new(p_start, points.len() as u32),
                colors: Span::new(c_start, colors.len() as u32),
                color_mode: mode,
                cap,
                join,
            },
            1.0,
        );
    }
}

pub(super) fn curve(b: &mut PaintCapture, bbox: Rect) {
    use crate::primitives::geometry::cubic_bezier::CubicBezier;
    use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
    use crate::shape::paint::curve_basis::CurveBasis;
    b.draw_curve(
        DrawCurvePayload {
            bounds: StrokeBounds::Still(bbox),
            origin: Vec2::ZERO,
            basis: CurveBasis::Cubic(CubicBezier {
                p0: bbox.min,
                p1: Vec2::new(bbox.min.x + bbox.size.w * 0.3, bbox.max().y),
                p2: Vec2::new(bbox.min.x + bbox.size.w * 0.7, bbox.max().y),
                p3: bbox.max(),
            }),
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
    b.draw_image(
        ImageDraw {
            payload: DrawImagePayload {
                rect: r,
                uv_min: Vec2::ZERO,
                uv_size: Vec2::ONE,
                tint: RgbaF32::WHITE.into(),
                handle: TextureId(1),
                flags: ImageFlags::NONE,
            },
            view: None,
        },
        1.0,
    );
}
