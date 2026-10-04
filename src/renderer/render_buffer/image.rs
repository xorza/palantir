//! Composited image and off-screen `GpuView` draw records.

#![expect(
    clippy::expl_impl_clone_on_copy,
    reason = "`soa_rs`'s `Soars` derive writes `Clone` by hand for the `Copy` rows it generates"
)]

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use glam::{UVec2, Vec2};
use soa_rs::Soars;

/// One `GpuView` off-screen target to paint this frame (see
/// [`RenderBuffer::frame_targets`](crate::renderer::render_buffer::RenderBuffer::frame_targets)):
/// the view's stable texture `id`, its used physical size (`used`), where
/// that sits in the view, the effective raster scale, and the app `paint`
/// callback (threaded from `Ui::gpu_views` through the typed image command,
/// so the backend reaches the renderer without a `Ui`-side registry). The
/// backend allocates the target to exactly `used` and runs `paint` into it
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
    /// contains-content rule in [`AxisSlot::resolve`](crate::layout) — so a
    /// view's rect can reach past the surface or past a scroll's viewport. The
    /// target follows what can be seen; this says what that is a part of, so a
    /// caller can still place its content against the whole.
    pub(crate) full: UVec2,
    /// Where `used` begins within `full`, in the same pixels.
    pub(crate) offset: UVec2,
    pub(crate) raster_scale: f32,
    pub(crate) paint: GpuPaintRef,
    /// The view's repaint version. See [`ViewStamp`].
    pub(crate) epoch: u64,
}

/// Everything a painted target's pixels depend on besides the callback,
/// which keys the target itself: the view's repaint version and the
/// geometry the paint was asked for. A target whose last paint carries
/// the same stamp holds this frame's pixels, so compositing it again
/// needs no paint — the case of a `repaint(false)` view under a partial
/// repaint that crosses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewStamp {
    epoch: u64,
    used: UVec2,
    full: UVec2,
    offset: UVec2,
    display_scale: f32,
    raster_scale: f32,
}

impl RenderTargetDraw {
    /// The stamp of this draw painted at the frame's `display_scale`.
    pub(crate) const fn stamp(&self, display_scale: f32) -> ViewStamp {
        ViewStamp {
            epoch: self.epoch,
            used: self.used,
            full: self.full,
            offset: self.offset,
            display_scale,
            raster_scale: self.raster_scale,
        }
    }
}

/// The frame's two views of its `GpuView`s, handed to the backend together
/// because keeping them apart is what the design turns on.
///
/// They answer different questions and neither implies the other:
/// [`Self::draws`] is what *changed* and has to be repainted,
/// [`Self::live`] is what still *exists*. Retention follows the second — a
/// view whose content is unchanged is culled out of the first and keeps its
/// off-screen texture, so sitting a frame out costs it nothing and does not
/// re-run `GpuPaint::init`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameViews<'a> {
    /// The views to paint this frame, each with its target geometry and the
    /// app callback that fills it.
    pub(crate) draws: &'a [RenderTargetDraw],
    /// Every view the frame recorded, painted or not — the retention roster.
    /// A superset of the ids in [`Self::draws`].
    pub(crate) live: &'a [TextureId],
    /// The frame's display scale, which every draw is painted at.
    pub(crate) display_scale: f32,
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
    pub(crate) uv_min: Vec2,
    /// UV crop extent (typically `(1, 1)`; smaller for `Cover` crop,
    /// `> 1` for `Tile` repeats). A `GpuView` ships `(1, 1)` so its entire
    /// target maps across the composite paint rect.
    pub(crate) uv_size: Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: RgbaF16,
    /// Tile wrap, min/mag nearest sampling and minification tap mode.
    pub(crate) flags: ImageFlags,
}
