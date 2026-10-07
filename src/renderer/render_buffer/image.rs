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
/// [`RenderBuffer::frame_targets`](crate::renderer::render_buffer::RenderBuffer::frame_targets)).
/// The backend allocates it to exactly `used` and runs `paint` before the
/// main pass samples it.
#[derive(Clone, Debug)]
pub(crate) struct RenderTargetDraw {
    pub(crate) id: TextureId,
    /// The target's size: the on-screen part of the view.
    pub(crate) used: UVec2,
    /// What the whole view measures, on and off screen, since layout may overflow.
    pub(crate) full: UVec2,
    /// Where `used` begins within `full`.
    pub(crate) offset: UVec2,
    pub(crate) raster_scale: f32,
    pub(crate) paint: GpuPaintRef,
    /// The view's repaint version; see [`ViewStamp`].
    pub(crate) epoch: u64,
}

/// Everything a painted target's pixels depend on besides the callback. A
/// target whose last paint carries the same stamp needs no repaint.
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

/// The frame's two views of its `GpuView`s: [`Self::draws`] changed and must
/// repaint, [`Self::live`] still exists. Retention follows the second.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameViews<'a> {
    /// The views to paint this frame.
    pub(crate) draws: &'a [RenderTargetDraw],
    /// Every view recorded, painted or not: the retention roster.
    pub(crate) live: &'a [TextureId],
    /// The display scale every draw is painted at.
    pub(crate) display_scale: f32,
}

/// One image draw row. SoA storage lets the backend upload `rows.instance()`
/// in one `write_buffer`; `id` is the registration id behind an `ImageHandle`.
#[derive(Soars, Clone, Copy, Debug, PartialEq)]
#[soa_derive(Debug)]
pub(crate) struct ImageDrawRow {
    pub(crate) id: TextureId,
    pub(crate) instance: ImageInstance,
}

/// Per-image GPU state. The shader samples at `uv_min + corner * uv_size` and
/// multiplies by `tint`; the UV pair carries the `ImageFit::Cover` crop.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageInstance {
    /// Physical-px paint rect.
    pub(crate) rect: Rect,
    /// UV crop top-left (0..1 texture coords).
    pub(crate) uv_min: Vec2,
    /// UV crop extent: `(1, 1)` normally, smaller for `Cover`, `> 1` for `Tile`.
    pub(crate) uv_size: Vec2,
    /// Linear-RGBA tint, premultiplied in the shader.
    pub(crate) tint: RgbaF16,
    /// Tile wrap, min/mag nearest sampling and minification tap mode.
    pub(crate) flags: ImageFlags,
}
