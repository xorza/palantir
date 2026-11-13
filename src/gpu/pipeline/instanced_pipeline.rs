//! [`InstancedPipeline`]: what the mesh, image and curve families share.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::atlas_binding::AtlasSlots;
use crate::gpu::pipeline::clip_variant::Clipping;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::PipelineRecipe;
use crate::gpu::resource::shader_body::ShaderBody;

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
    /// The pipeline layout the family's pipelines build over.
    pub(crate) layout: &'a wgpu::PipelineLayout,
    /// The instance buffer's label.
    pub(crate) instances: &'static str,
    /// The instances the buffer holds before its first grow.
    pub(crate) capacity: usize,
}

/// What a pipeline is built for: the target's color format, whether the frame's draws name
/// rounded clips, and the open page slots of the pool's lists they read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VariantTarget {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) clipping: Clipping,
    pub(crate) slots: AtlasSlots,
}

/// What picks a frame's pipelines of every format, made once a submit after the pool's commit:
/// whether its draws name rounded clips, and the open page slots of the lists its raster and
/// shadow draws read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameVariant {
    pub(crate) clipping: Clipping,
    pub(crate) slots: AtlasSlots,
}

impl FrameVariant {
    /// What a frame of this variant builds its pipelines of `format` for.
    pub(crate) const fn target(self, format: wgpu::TextureFormat) -> VariantTarget {
        VariantTarget {
            format,
            clipping: self.clipping,
            slots: self.slots,
        }
    }
}

impl<I: bytemuck::Pod> InstancedPipeline<I> {
    pub(crate) fn new(gpu: &Gpu, spec: InstancedSpec<'_>) -> Self {
        Self {
            instances: DynamicBuffer::vertex(gpu, spec.instances, spec.capacity),
            shader: spec.shader.module(gpu),
            layout: spec.layout.clone(),
        }
    }

    /// The family's color pipeline for `format`, over its shader and layout, which a family
    /// overrides what it varies of with struct update.
    pub(crate) const fn recipe(
        &self,
        label: &'static str,
        format: wgpu::TextureFormat,
    ) -> PipelineRecipe<'_> {
        PipelineRecipe::color(label, &self.shader, &self.layout, format)
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
