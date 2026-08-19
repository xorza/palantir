//! One textured-quad draw.

use crate::primitives::color::ColorF16;
use crate::primitives::rect::Rect;
use crate::primitives::texture_id::TextureId;

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
    pub(crate) tint: ColorF16,
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

impl DrawImagePayload {
    /// An image draw.
    #[inline]
    pub(crate) fn image(
        rect: Rect,
        uv_min: glam::Vec2,
        uv_size: glam::Vec2,
        tint: ColorF16,
        handle: TextureId,
        flags: u32,
    ) -> Self {
        Self {
            rect,
            uv_min,
            uv_size,
            tint,
            handle,
            flags,
        }
    }

    /// Paints nothing when: zero-extent rect, fully transparent tint,
    /// or null handle (paints no pixels, no texture to sample).
    ///
    /// `is_gpu_view` rather than a field, because it is the same fact as
    /// the `paint` callback the sink already carries beside the payload —
    /// a `GpuView` is never null-skipped, since its texture is
    /// framework-painted this frame rather than a registered image that
    /// could have been dropped. Held on the payload it would ride in
    /// every `PartialEq` and every captured call for one read, two lines
    /// after the write.
    #[inline]
    pub(crate) fn is_noop(&self, is_gpu_view: bool) -> bool {
        self.rect.is_paint_empty() || self.tint.is_noop() || (self.handle.0 == 0 && !is_gpu_view)
    }
}
