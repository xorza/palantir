//! Drawing the retained backbuffer onto a target that cannot be copied into.

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::resource::texture_binding::TextureBinding;

/// The format-independent half of the backbuffer blit: one shader module, no buffers.
///
/// The peer of [`Backbuffer::copy_onto`](crate::gpu::surface::backbuffer::Backbuffer::copy_onto) for targets lacking `COPY_DST` (a GLES swapchain image takes draws alone); without it such a surface would repaint whole instead of damage-limited. It binds the shared group-0 layout; its sampler slot goes unused, since the shader reads texels by index (a filtered read isn't a copy).
#[derive(Debug)]
pub(crate) struct BlitPipeline {
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once.
    pipeline_layout: wgpu::PipelineLayout,
}

impl BlitPipeline {
    pub(crate) fn new(device: &wgpu::Device, textures: &TextureBinding) -> Self {
        let shader = ShaderBody::Blit.module(device);
        Self {
            shader,
            pipeline_layout: PipelineRecipe::pipeline_layout(
                device,
                "palantir.blit.pl",
                &[Some(textures.layout())],
            ),
        }
    }

    /// The pipeline for one target format. No blend: the backbuffer holds the finished frame, so the draw replaces the target. No stencil twin: it runs after every clipped draw, in its own pass.
    pub(super) fn build(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        PipelineRecipe {
            label: "palantir.blit.pipeline",
            shader: &self.shader,
            layout: &self.pipeline_layout,
            vertex_entry: "vs",
            constants: &[],
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
