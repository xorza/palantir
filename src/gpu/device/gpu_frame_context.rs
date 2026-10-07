//! What a `GpuPaint` gets on every painted frame.

use glam::UVec2;
use std::fmt;
use std::time::Duration;

/// Handed to [`GpuPaint::paint`](crate::renderer::gpu_paint::GpuPaint::paint) each painted frame.
pub struct GpuFrameContext<'a> {
    /// The device.
    pub device: &'a wgpu::Device,
    /// The queue.
    pub queue: &'a wgpu::Queue,
    /// Palantir's main command encoder: record your render pass(es) here.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The off-screen color target, sized exactly to [`Self::physical_size`]; render into the whole target.
    pub target: &'a wgpu::TextureView,
    /// The target's actual size in physical pixels, after the widget's composed transform and downsampling for the device texture cap. Size your viewport and attachments to it; the target is reallocated whenever it changes.
    ///
    /// **What is on screen, not always the whole view**: a widget's rect may reach past the window or a scroll's pane, and nothing is allocated for the unseen part. Then [`Self::physical_full_size`] is the whole and [`Self::physical_offset`] places this within it.
    pub physical_size: UVec2,
    /// What the whole view measures in the same pixels (equal to [`Self::physical_size`] when nothing clips). Derive your projection's shape from this: it does not change as a scroll slides the view past its pane.
    pub physical_full_size: UVec2,
    /// Where [`Self::physical_size`] begins within [`Self::physical_full_size`]; `ZERO` when nothing clips. Skew your projection by it, as a tile renderer does. **Not optional for a picked view**: hit-testing is reported against `physical_full_size`, so framing to `physical_size` would disagree and appear to zoom as a scroll slides.
    pub physical_offset: UVec2,
    /// Logical→display scale for this window's monitor; display pixel density only.
    pub display_scale: f32,
    /// Logical→target scale for this view, including display scale, composed transforms and device-cap downsampling.
    pub raster_scale: f32,
    /// Wall-clock time since this view last painted (`Duration::ZERO` on its first), for framerate-independent animation.
    pub dt: Duration,
}

impl fmt::Debug for GpuFrameContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GpuFrameContext")
            .field("physical_size", &self.physical_size)
            .field("display_scale", &self.display_scale)
            .field("raster_scale", &self.raster_scale)
            .field("dt", &self.dt)
            .finish_non_exhaustive()
    }
}
