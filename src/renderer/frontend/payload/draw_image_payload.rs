//! One textured-quad draw, and the pair a sink takes it as.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::image_flags::ImageFlags;

/// Image draw payload: `rect` is the logical-px paint rect (`fit` and intrinsic size folded in),
/// `uv_min`/`uv_size` crop the texture (non-trivial only for Cover), `tint` multiplies the texel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawImagePayload {
    pub(crate) rect: Rect,
    pub(crate) uv_min: glam::Vec2,
    pub(crate) uv_size: glam::Vec2,
    pub(crate) tint: RgbaF16,
    /// Registration id keying the backend's texture cache; `TextureId(0)` means no texture and skips the draw.
    pub(crate) handle: TextureId,
    /// Tile wrap, nearest sampling and minification tap mode, forwarded to
    /// [`ImageInstance::flags`](crate::renderer::render_buffer::image::ImageInstance).
    /// [`ImageFlags::NONE`] takes one bilinear tap.
    pub(crate) flags: ImageFlags,
}

/// One image draw as [`PaintSink::image`] takes it: the payload plus the paint callback of a `GpuView`.
///
/// One value so the two cannot come apart; borrowed so the draw stays `Copy`.
///
/// [`PaintSink::image`]: crate::renderer::frontend::paint_sink::PaintSink::image
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageDraw<'a> {
    pub(crate) payload: DrawImagePayload,
    pub(crate) view: Option<ViewPaint<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewPaint<'a> {
    pub(crate) paint: &'a GpuPaintRef,
    /// The view's repaint version; a target painted at this version and geometry is still valid.
    pub(crate) epoch: u64,
}

impl ImageDraw<'_> {
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            payload: DrawImagePayload {
                tint: self.payload.tint.faded(by),
                ..self.payload
            },
            ..self
        }
    }

    /// Paints nothing: zero-extent rect, transparent tint, or a null handle with no callback.
    #[inline]
    pub(crate) const fn is_noop(&self) -> bool {
        let Self { payload, view } = self;
        payload.rect.is_paint_empty()
            || payload.tint.is_noop()
            || (payload.handle.0 == 0 && view.is_none())
    }
}
