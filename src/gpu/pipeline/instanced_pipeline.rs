//! [`InstancedPipeline`]: what the mesh, image and curve families share.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe};
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;

/// A drawing family that draws one frame's instances of `I`: their buffer, uploaded each frame,
/// and the shader and pipeline layout every format's pipelines build over.
#[derive(Debug)]
pub(crate) struct InstancedPipeline<I: bytemuck::Pod> {
    instances: DynamicBuffer<I>,
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
}

/// What an [`InstancedPipeline`] is made of.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InstancedSpec<'a> {
    pub(crate) shader: ShaderBody,
    /// The pipeline layout's label.
    pub(crate) layout: &'static str,
    /// The bind group layouts the pipelines read, by group index.
    pub(crate) groups: &'a [Option<&'a wgpu::BindGroupLayout>],
    /// The instance buffer's label.
    pub(crate) instances: &'static str,
    /// The instances the buffer holds before its first grow.
    pub(crate) capacity: usize,
}

/// How a family's pipelines differ from [`PipelineRecipe::color`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct VariantRecipe<'a> {
    pub(crate) label: &'static str,
    pub(crate) stencil_label: &'static str,
    pub(crate) vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(crate) topology: wgpu::PrimitiveTopology,
}

impl<I: bytemuck::Pod> InstancedPipeline<I> {
    pub(crate) fn new(gpu: &Gpu, spec: InstancedSpec<'_>) -> Self {
        Self {
            instances: DynamicBuffer::vertex(gpu, spec.instances, spec.capacity),
            shader: spec.shader.module(gpu),
            layout: pipeline_recipe::pipeline_layout(gpu, spec.layout, spec.groups),
        }
    }

    /// The base and stencil-test color pipelines of `recipe` against `format`, the only
    /// format-dependent objects of the family.
    pub(crate) fn build_variants(
        &self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        recipe: VariantRecipe<'_>,
    ) -> StencilVariant {
        StencilVariant::build(
            gpu,
            PipelineRecipe {
                vertex_buffers: recipe.vertex_buffers,
                topology: recipe.topology,
                ..PipelineRecipe::color(recipe.label, &self.shader, &self.layout, format)
            },
            recipe.stencil_label,
        )
    }

    /// Upload this frame's `instances`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[I]) {
        self.instances.upload_instances(ctx, instances);
    }

    /// The buffer of this frame's instances.
    pub(crate) const fn instances(&self) -> &wgpu::Buffer {
        &self.instances.buffer
    }

    pub(crate) const fn layout(&self) -> &wgpu::PipelineLayout {
        &self.layout
    }
}
