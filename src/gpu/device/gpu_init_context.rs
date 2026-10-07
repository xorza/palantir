//! What a `GpuPaint` gets once, to build its pipelines.

use crate::text::shaper::TextShaper;

/// Handed once to [`GpuPaint::init`](crate::renderer::gpu_paint::GpuPaint::init).
#[derive(Debug)]
pub struct GpuInitContext<'a> {
    /// The device to create pipelines and resources against.
    pub device: &'a wgpu::Device,
    /// The off-screen color target's format (`Rgba8UnormSrgb`); match it in your pipeline.
    pub target_format: wgpu::TextureFormat,
    /// The window's text shaper, so a view's own text agrees with the UI around it.
    ///
    /// Clone and keep it; take a lease per batch in [`GpuPaint::paint`](crate::renderer::gpu_paint::GpuPaint::paint).
    pub text: &'a TextShaper,
}
