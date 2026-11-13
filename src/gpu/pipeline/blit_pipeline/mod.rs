//! Drawing the retained backbuffer onto a target that cannot be copied into.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::pipeline_recipe::{self, PipelineRecipe};
use crate::gpu::resource::shader_body::ShaderBody;
use crate::gpu::resource::texture_binding::TextureBinding;
use rustc_hash::FxHashMap;

/// The backbuffer blit: one shader module and layout, and the pipeline built for each target
/// format asked for, the screenshot's [`Capture::FORMAT`](crate::gpu::surface::capture::Capture::FORMAT)
/// among them. Refused: a blit in each format's pipeline set beside one for the capture, two
/// caches of one pipeline that built it twice for an sRGB RGBA target.
///
/// What [`Backbuffer::present_onto`](crate::gpu::surface::backbuffer::Backbuffer::present_onto) draws in place of a copy, for targets lacking `COPY_DST` (a GLES swapchain image takes draws alone); without it such a surface would repaint whole instead of damage-limited. It binds the shared group-0 layout; its sampler slot goes unused, since the shader reads texels by index (a filtered read isn't a copy).
#[derive(Debug)]
pub(crate) struct BlitPipeline {
    shader: wgpu::ShaderModule,
    /// Format-independent, so built once.
    pipeline_layout: wgpu::PipelineLayout,
    /// One pipeline per target format; a backend meets a few formats, so the map stays small.
    pipelines: FxHashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl BlitPipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.blit.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[],
        constants: &[],
    };

    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding) -> Self {
        let shader = Self::SHADER.module(gpu);
        Self {
            shader,
            pipeline_layout: pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.blit.pl",
                &[Some(textures.texture_layout())],
            ),
            pipelines: FxHashMap::default(),
        }
    }

    /// Build the pipeline for `format` where none is built. No blend: the backbuffer holds the
    /// finished frame, so the draw replaces the target, in its own pass.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, format: wgpu::TextureFormat) {
        if self.pipelines.contains_key(&format) {
            return;
        }
        let pipeline = PipelineRecipe {
            blend: None,
            ..PipelineRecipe::color(
                "palantir.blit.pipeline",
                &self.shader,
                &self.pipeline_layout,
                format,
            )
        }
        .build(gpu)
        .pipeline;
        self.pipelines.insert(format, pipeline);
    }

    /// The pipeline for `format`, which [`Self::ensure`] built.
    pub(crate) fn get(&self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        self.pipelines
            .get(&format)
            .unwrap_or_else(|| panic!("a blit into {format:?} is built before it draws"))
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
