//! What a `GpuPaint` gets on every painted frame.

use glam::UVec2;
use std::time::Duration;

/// Handed to [`GpuPaint::paint`](crate::renderer::gpu_paint::GpuPaint::paint) each painted frame.
pub struct GpuFrameCtx<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// Palantir's main command encoder — record your render pass(es) here.
    /// wgpu inserts the `RENDER_ATTACHMENT → TEXTURE_BINDING` transition
    /// between your pass and the main pass that samples `target`.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The off-screen color target, sized exactly to [`Self::size_px`]. Set
    /// your viewport/scissor to `size_px` and render into the whole target.
    pub target: &'a wgpu::TextureView,
    /// The target's actual size in physical pixels, after the widget's composed
    /// transform and any uniform downsampling required by the device texture
    /// cap. Set your viewport to this and size your own attachments (depth,
    /// MSAA) to it — the target is reallocated whenever this changes (every
    /// frame while the view is being resized).
    ///
    /// **What is on screen, which is not always the whole view.** A widget's
    /// rect may reach past the window or past the pane a scroll clips it to,
    /// and nothing is allocated for the part that cannot be seen. Where they
    /// differ, [`Self::full_px`] is the whole and [`Self::offset_px`] says where
    /// this sits in it.
    pub size_px: UVec2,
    /// What the whole view measures, in the same pixels — equal to
    /// [`Self::size_px`] whenever nothing clips the view, which is the usual
    /// case.
    ///
    /// Derive your projection's *shape* from this rather than from `size_px`:
    /// it is the aspect the view was laid out at, and it does not change as a
    /// scroll slides the view past its pane.
    pub full_px: UVec2,
    /// Where [`Self::size_px`] begins within [`Self::full_px`], in the same
    /// pixels. `ZERO` whenever nothing clips the view.
    ///
    /// Treat this and `size_px` as a window onto `full_px` and skew your
    /// projection by it, the way a tile renderer does.
    ///
    /// **Not optional for a view that is also picked.** A renderer that framed
    /// its content to `size_px` instead would draw a different picture from the
    /// one its own hit-testing assumes — the widget's rect is still the whole
    /// view, so a cursor is reported against `full_px` — and the two would
    /// disagree by however much is clipped. It would also reframe as a scroll
    /// slid the view past its pane, which reads as the content zooming.
    pub offset_px: UVec2,
    /// Logical→display scale for this window's current monitor. This is the
    /// display pixel density only; widget transforms and target downsampling do
    /// not affect it.
    pub display_scale: f32,
    /// Logical→target scale for this view, including the display scale,
    /// composed transforms, and any uniform device-cap downsampling.
    pub raster_scale: f32,
    /// Wall-clock time since this view last painted (`Duration::ZERO` on
    /// its first paint). Use it to make animation framerate-independent.
    pub dt: Duration,
}

impl std::fmt::Debug for GpuFrameCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuFrameCtx")
            .field("size_px", &self.size_px)
            .field("display_scale", &self.display_scale)
            .field("raster_scale", &self.raster_scale)
            .field("dt", &self.dt)
            .finish_non_exhaustive()
    }
}
