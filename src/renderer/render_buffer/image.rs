//! Composited image and off-screen `GpuView` draw records.

use crate::primitives::color::ColorU8;
use crate::primitives::rect::Rect;
use crate::primitives::texture_id::TextureId;
use crate::renderer::gpu_view::GpuPaintRef;
use glam::UVec2;
use soa_rs::Soars;

/// One `GpuView` off-screen target to paint this frame (see
/// [`RenderBuffer::frame_targets`](crate::renderer::render_buffer::RenderBuffer::frame_targets)):
/// the view's stable texture `id`, its used
/// physical size (`used`), where that sits in the view, the display and
/// effective raster scales, and the app
/// `paint` callback (threaded from `Ui::gpu_views` through the typed image
/// command, so the backend reaches the renderer without a `Ui`-side registry).
/// The backend allocates the target to exactly `used` and runs `paint` into it
/// before the main pass samples it.
#[derive(Clone, Debug)]
pub(crate) struct RenderTargetDraw {
    pub(crate) id: TextureId,
    /// The target's size: the part of the view that is actually on screen,
    /// which is the whole of it whenever nothing clips the view.
    pub(crate) used: UVec2,
    /// What the whole view measures, on screen and off.
    ///
    /// Apart from `used` because layout is *allowed* to overflow — see the
    /// contains-content rule in [`resolve_axis_size`](crate::layout) — so a
    /// view's rect can reach past the surface or past a scroll's viewport. The
    /// target follows what can be seen; this says what that is a part of, so a
    /// caller can still place its content against the whole.
    pub(crate) full: UVec2,
    /// Where `used` begins within `full`, in the same pixels.
    pub(crate) offset: UVec2,
    pub(crate) display_scale: f32,
    pub(crate) raster_scale: f32,
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
    pub(crate) id: TextureId,
    pub(crate) instance: ImageInstance,
}

/// Bit in [`ImageInstance::flags`]: wrap UVs with `fract` in the shader
/// (`ImageFit::Tile`).
pub(crate) const IMG_FLAG_TILED: u32 = 1 << 0;
/// Bit in [`ImageInstance::flags`]: nearest-neighbour minification.
pub(crate) const IMG_FLAG_MIN_NEAREST: u32 = 1 << 1;
/// Bit in [`ImageInstance::flags`]: nearest-neighbour magnification.
pub(crate) const IMG_FLAG_MAG_NEAREST: u32 = 1 << 2;
/// Bit in [`ImageInstance::flags`]: where this image minifies, spread a grid
/// of taps over the fragment's source footprint and average them
/// ([`ImageDownsample::Mean`](crate::ImageDownsample::Mean)).
pub(crate) const IMG_FLAG_TAPS_MEAN: u32 = 1 << 3;
/// Bit in [`ImageInstance::flags`]: as [`IMG_FLAG_TAPS_MEAN`], but the
/// brightest tap wins instead of the average
/// ([`ImageDownsample::Peak`](crate::ImageDownsample::Peak)). Mutually
/// exclusive with it — the encoder sets at most one.
pub(crate) const IMG_FLAG_TAPS_PEAK: u32 = 1 << 4;

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
    /// `> 1` for `Tile` repeats). A `GpuView` ships `(1, 1)` so its entire
    /// target maps across the composite paint rect.
    pub(crate) uv_size: glam::Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: ColorU8,
    /// `IMG_FLAG_*` bits (tile wrap, min/mag nearest sampling, minification
    /// tap mode). `u32` for a clean `Uint32` vertex attr.
    pub(crate) flags: u32,
}
