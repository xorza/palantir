//! What a `GpuPaint` gets once, to build its pipelines.

use crate::gpu::device::gpu::Gpu;
use crate::text::shaper::TextShaper;

/// Handed once to [`GpuPaint::init`](crate::gpu::gpu_paint::GpuPaint::init).
#[derive(Debug)]
#[non_exhaustive]
pub struct GpuInitCtx<'a> {
    /// The device to create resources against, through [`Gpu::device`], and pipelines, through
    /// [`Gpu::create_render_pipeline`] and [`Gpu::create_compute_pipeline`].
    pub gpu: &'a Gpu,
    /// The off-screen color target's format (`Rgba8UnormSrgb`); match it in your pipeline.
    pub target_format: wgpu::TextureFormat,
    /// The window's text shaper, so a view's own text agrees with the UI around it.
    ///
    /// Clone and keep it; take a lease per batch in [`GpuPaint::paint`](crate::gpu::gpu_paint::GpuPaint::paint).
    pub text: &'a TextShaper,
}
