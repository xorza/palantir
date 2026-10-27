//! Off-screen `GpuView` targets a frame paints, and the roster that keeps them alive.

use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use glam::UVec2;

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

/// The frame's two views of its `GpuView`s: [`Self::draws`] were composited, and the backend
/// repaints each whose stamp changed; [`Self::live`] still exists. Retention follows the second.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameViews<'a> {
    /// The views composited this frame, each repainted only when its stamp changed.
    pub(crate) draws: &'a [RenderTargetDraw],
    /// Every view recorded, painted or not: the retention roster.
    pub(crate) live: &'a [TextureId],
    /// The display scale every draw is painted at.
    pub(crate) display_scale: f32,
}
