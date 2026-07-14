//! Composited image and off-screen `GpuView` draw records.

use crate::primitives::color::ColorU8;
use crate::primitives::rect::Rect;
use crate::renderer::gpu_view::GpuPaintRef;
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use soa_rs::Soars;

/// One `GpuView` off-screen target to paint this frame (see
/// [`RenderBuffer::frame_targets`]): the view's stable texture `id`, its used
/// physical size (`used` — the composed paint-rect size, ceiled ≥1, clamped
/// to the device max), and the app `paint` callback (threaded from
/// `Ui::gpu_views` through the cmd-buffer side-list, so the backend reaches the
/// renderer without a `Ui`-side registry). The backend allocates the target to
/// exactly `used` and runs `paint` into it before the main pass samples it.
#[derive(Clone, Debug)]
pub(crate) struct RenderTargetDraw {
    pub(crate) id: TextureId,
    pub(crate) used: UVec2,
    pub(crate) paint: GpuPaintRef,
}

/// One image draw row. Composer pushes one of these per image; the
/// SoA storage splits `id` and `instance` into their own contiguous
/// slices, so the backend uploads `rows.instance()` as a single
/// `write_buffer` and walks `rows.id()` for per-draw texture bindings.
/// `id` is the registration id behind an `ImageHandle`; the backend
/// looks it up in its GPU texture cache (and skips the draw on a miss).
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub id: TextureId,
    pub instance: ImageInstance,
}

/// Bit in [`ImageInstance::flags`]: wrap UVs with `fract` in the shader
/// (`ImageFit::Tile`).
pub(crate) const IMG_FLAG_TILED: u32 = 1 << 0;
/// Bit in [`ImageInstance::flags`]: nearest-neighbour sampling
/// (`ImageFilter::Nearest`) — the shader snaps the UV to the texel
/// center before the (linear-sampler) fetch, which lands the bilinear
/// weights exactly on one texel.
pub(crate) const IMG_FLAG_NEAREST: u32 = 1 << 1;

/// Per-image GPU state, uploaded to a `step_mode: Instance` vertex
/// buffer. Shader interpolates `uv_min + corner * uv_size` per fragment
/// (where `corner` is the four-corner `vertex_index`), samples the
/// texture, and multiplies by `tint`. `uv_min`+`uv_size` carry the
/// crop for `ImageFit::Cover`; the other fit modes ship `(0,0)+(1,1)`
/// and let the encoder shape the paint rect instead. `Pod`-shaped so
/// the upload is a single `write_buffer`.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    /// Physical-px paint rect.
    pub(crate) rect: Rect,
    /// UV crop top-left (0..1 texture coords).
    pub(crate) uv_min: glam::Vec2,
    /// UV crop extent (typically `(1, 1)`; smaller for `Cover` crop,
    /// `> 1` for `Tile` repeats). A `GpuView` ships `(1, 1)` — its target is
    /// sized exactly to the paint rect, so it samples the whole texture.
    pub(crate) uv_size: glam::Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: ColorU8,
    /// `IMG_FLAG_*` bits (tile wrap, nearest sampling). `u32` for a
    /// clean `Uint32` vertex attr.
    pub(crate) flags: u32,
}
