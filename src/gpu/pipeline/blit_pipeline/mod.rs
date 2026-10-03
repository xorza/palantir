//! Drawing the retained backbuffer onto a target, for targets that cannot be
//! copied into.

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;

/// The format-independent half of the backbuffer blit: one shader module,
/// no buffers.
///
/// The peer of [`Backbuffer::copy_onto`](crate::gpu::surface::backbuffer::Backbuffer::copy_onto),
/// reached when the target lacks `COPY_DST` — a GLES swapchain image, which is
/// the default framebuffer and takes draws alone. Without it such a surface
/// would have to give up damage-limited painting and repaint whole.
///
/// It binds the same group-0 layout every image draw uses, rather than a
/// second one that would have to agree with it. The sampler slot that layout
/// carries goes unused: the shader reads texels by index, because this stands
/// in for a copy and a filtered read is not one.
#[derive(Debug)]
pub(crate) struct BlitPipeline {
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
}

impl BlitPipeline {
    pub(crate) fn new(device: &wgpu::Device, image_bgl: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.blit.shader"),
            source: wgpu::ShaderSource::Wgsl(ShaderBody::Blit.specialize(&[]).into()),
        });
        Self {
            shader,
            pipeline_layout: PipelineRecipe::pipeline_layout(
                device,
                "palantir.blit.pl",
                &[Some(image_bgl)],
            ),
        }
    }

    /// The pipeline for one target format.
    ///
    /// No blend state: the backbuffer holds the finished frame, so the draw
    /// replaces the target rather than compositing onto it. No stencil twin
    /// either — this runs after every clipped draw, in a pass of its own.
    pub(super) fn build(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        PipelineRecipe {
            label: "palantir.blit.pipeline",
            shader: &self.shader,
            layout: &self.pipeline_layout,
            vertex_buffers: &[],
            topology: wgpu::PrimitiveTopology::TriangleList,
            color_format: format,
            fragment_entry: "fs",
            color_writes: wgpu::ColorWrites::ALL,
            blend: None,
            depth_stencil: None,
        }
        .build(device)
    }
}
