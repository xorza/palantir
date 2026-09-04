//! One textured-quad draw, and the pair a sink takes it as.

use crate::primitives::color::RgbaF16;
use crate::primitives::rect::Rect;
use crate::primitives::texture_id::TextureId;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;

/// Image draw payload. `rect` is the logical-px paint rect (encoder
/// already folded in `local_rect`, `fit`, and the image's intrinsic
/// size). `uv_min` / `uv_size` are the texture crop — `(0,0)`+`(1,1)`
/// for the common Fill/Contain/None modes; non-trivial only for Cover.
/// `tint` multiplies the sampled texel. `handle` is the user-supplied
/// [`ImageHandle`](crate::renderer::image_registry::ImageHandle) — the
/// backend looks it up against its GPU texture
/// cache.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawImagePayload {
    pub(crate) rect: Rect,
    pub(crate) uv_min: glam::Vec2,
    pub(crate) uv_size: glam::Vec2,
    pub(crate) tint: RgbaF16,
    /// The image's registration id ([`TextureId`],
    /// a `repr(transparent)` `Pod` `u64`). The backend looks it up in its
    /// texture cache; `TextureId(0)` (the `Zeroable` default) is "no
    /// texture" and skips the draw.
    pub(crate) handle: TextureId,
    /// `IMG_FLAG_*` bits (tile wrap, min/mag nearest sampling, minification
    /// tap mode), forwarded
    /// verbatim into [`ImageInstance::flags`](crate::renderer::render_buffer::image::ImageInstance).
    /// `0` (the common case, including a `GpuView`) takes one bilinear tap at
    /// the UV.
    pub(crate) flags: u32,
}

/// One image draw as [`PaintSink::image`] takes it: the payload plus, for
/// a `GpuView` composite, the callback its off-screen target is painted
/// with.
///
/// One value rather than two arguments, so the composite and the target
/// it needs cannot come apart and the no-op question reads the same
/// `is_noop(&self)` every sibling payload answers. Borrowed rather than
/// owned so the draw stays `Copy` — the `Rc` clone is the capture sink's
/// to pay, once, when it keeps a call past the frame.
///
/// [`PaintSink::image`]: crate::renderer::frontend::paint_sink::PaintSink::image
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageDraw<'a> {
    pub(crate) payload: DrawImagePayload,
    pub(crate) paint: Option<&'a GpuPaintRef>,
}

impl ImageDraw<'_> {
    /// Paints nothing when: zero-extent rect, fully transparent tint, or
    /// a null handle with no callback behind it — a registered image
    /// that was dropped. A `GpuView` is never null-skipped, since its
    /// texture is framework-painted this frame.
    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        let Self { payload, paint } = self;
        payload.rect.is_paint_empty()
            || payload.tint.is_noop()
            || (payload.handle.0 == 0 && paint.is_none())
    }
}
