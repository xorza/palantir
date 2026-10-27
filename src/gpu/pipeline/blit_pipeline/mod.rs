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
    pub(crate) fn build(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        PipelineRecipe {
            blend: None,
            ..PipelineRecipe::color(
                "palantir.blit.pipeline",
                &self.shader,
                &self.pipeline_layout,
                format,
            )
        }
        .build(device)
    }
}

/// Draw the texture `source` binds onto `target`, texel for texel, in a pass of its own since it
/// replaces the target.
pub(crate) fn draw(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::BindGroup,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("palantir.renderer.blit"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                // Clear, though every texel is written: on tilers a clear skips loading tile memory.
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, source, &[]);
    pass.draw(0..3, 0..1);
}
