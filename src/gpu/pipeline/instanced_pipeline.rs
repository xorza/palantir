//! [`InstancedPipeline`]: what the mesh, image and curve families share.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe};
use crate::gpu::pipeline::shader_body::ShaderBody;
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

/// What a pipeline is built for: the target's color format, and whether the frame clips to a
/// rounded rect.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VariantTarget {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) clipping: Clipping,
}

/// How a family's pipelines differ from [`PipelineRecipe::color`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct VariantRecipe<'a> {
    pub(crate) label: &'static str,
    pub(crate) vertex_entry: &'static str,
    pub(crate) fragment_entry: &'static str,
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

    /// Build the half of `variant` `target` binds, `recipe` against its format, where it is not
    /// built: the only format-dependent objects of the family.
    pub(crate) fn ensure(
        &self,
        variant: &mut ClipVariant,
        gpu: &Gpu,
        target: VariantTarget,
        recipe: VariantRecipe<'_>,
    ) {
        variant.ensure(
            gpu,
            target.clipping,
            &PipelineRecipe {
                vertex_entry: recipe.vertex_entry,
                fragment_entry: recipe.fragment_entry,
                vertex_buffers: recipe.vertex_buffers,
                topology: recipe.topology,
                ..PipelineRecipe::color(recipe.label, &self.shader, &self.layout, target.format)
            },
        );
    }

    /// Upload this frame's `instances`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[I]) {
        self.instances.upload_instances(ctx, instances);
    }

    /// The buffer of this frame's instances.
    pub(crate) const fn instances(&self) -> &wgpu::Buffer {
        &self.instances.buffer
    }
}
