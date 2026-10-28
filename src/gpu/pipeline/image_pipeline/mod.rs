//! GPU side of user images: like [`crate::gpu::pipeline::mesh_pipeline::MeshPipeline`]
//! but draws textured quads (per-instance rect + tint, per-image bind group).
//!
//! Bind groups belong to the two stores the backend hands
//! [`ImagePipeline::draw`]: [`WgpuImageStore`](crate::gpu::resource::wgpu_image_store::WgpuImageStore)
//! and [`GpuViewTargets`]. Both build against one binding shape, so a view
//! composite binds like an image.

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_view_targets::GpuViewTargets;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::ImageTexture;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::render_buffer::image_draw_row::ImageInstance;
use rustc_hash::FxHashMap;

/// One batch of image draws: the frame's per-draw texture column and this
/// batch's slice of it. `items` is the same [`Span`] that indexes the instance
/// buffer, so textures and instances can't come from different batches.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageBatch<'a> {
    pub(crate) ids: &'a [TextureId],
    pub(crate) items: Span,
}

/// The two stores an image draw's texture id resolves in: registered images first, as the common
/// draw, then `GpuView` targets.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageTextures<'a> {
    pub(crate) images: &'a FxHashMap<TextureId, ImageTexture>,
    pub(crate) targets: &'a GpuViewTargets,
}

impl<'a> ImageTextures<'a> {
    /// The bind group `id` samples through, or `None` once its owner dropped it.
    fn bind_group(self, id: TextureId) -> Option<&'a wgpu::BindGroup> {
        self.images
            .get(&id)
            .map(|entry| &entry.bind_group)
            .or_else(|| self.targets.bind_group(id))
    }
}

#[derive(Debug)]
pub(crate) struct ImagePipeline {
    instance_buffer: DynamicBuffer<ImageInstance>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
}

impl ImagePipeline {
    /// Format-independent resources: shader and instance buffer. Pipelines come
    /// from [`Self::build_variants`] via
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(device: &wgpu::Device, textures: &TextureBinding) -> Self {
        let shader = ShaderBody::Image.module(device);

        let instance_buffer =
            DynamicBuffer::<ImageInstance>::vertex(device, "palantir.image.instances", 16);

        Self {
            instance_buffer,
            shader,
            pipeline_layout: PipelineRecipe::pipeline_layout(
                device,
                "palantir.image.pl",
                &[Some(textures.layout())],
            ),
        }
    }

    pub(super) const fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<ImageInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &IMAGE_INSTANCE_ATTRS,
        }
    }

    /// Build the base and stencil-test color pipelines against `format`, the only
    /// format-dependent image objects. Called by `FormatPipelines` per format.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        StencilVariant::build(
            device,
            PipelineRecipe {
                vertex_buffers: &[Some(Self::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..PipelineRecipe::color(
                    "palantir.image.pipeline",
                    &self.shader,
                    &self.pipeline_layout,
                    format,
                )
            },
            "palantir.image.pipeline.stencil_test",
        )
    }

    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[ImageInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    pub(crate) fn bind<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        pipeline: &'a wgpu::RenderPipeline,
    ) {
        binder.pipeline(pass, pipeline, &self.pipeline_layout);
        binder.vertex(pass, 0, &self.instance_buffer.buffer);
    }

    /// Draw one image batch. `ids` is the frame's per-draw texture column and
    /// `items` selects this batch's slice (see [`ImageBatch`]).
    ///
    /// Adjacent draws sharing a texture collapse into one `set_bind_group` +
    /// instanced `draw` (see [`Runs`]).
    ///
    /// An **absent id is skipped** silently: its owning
    /// [`ImageHandle`](crate::ImageHandle) was dropped. The check runs once per run.
    pub(crate) fn draw<'a>(
        &self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        ImageBatch { ids, items }: ImageBatch<'_>,
        textures: ImageTextures<'_>,
    ) {
        binder.forget_group(0);
        for run in Runs::new(items, |i| ids[i as usize]) {
            let Some(bind_group) = textures.bind_group(run.key) else {
                continue;
            };
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..4, run.instances.into());
        }
    }
}

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
    0 => Float32x2,
    1 => Float32x2,
    2 => Float32x2,
    3 => Float32x2,
    // Tint is linear straight-alpha; the shader premultiplies at write.
    4 => Float16x4,
    5 => Uint32,
    6 => Float32,
];

// Attribute offsets must match the `ImageInstance` fields; `offset_of!`
// catches a same-size reorder that `array_stride == size_of` would miss.
const _: () = {
    use std::mem::offset_of;
    assert!(IMAGE_INSTANCE_ATTRS[0].offset == offset_of!(ImageInstance, rect.min) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[1].offset == offset_of!(ImageInstance, rect.size) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[2].offset == offset_of!(ImageInstance, uv_min) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[3].offset == offset_of!(ImageInstance, uv_size) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[4].offset == offset_of!(ImageInstance, tint) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[5].offset == offset_of!(ImageInstance, flags) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[6].offset == offset_of!(ImageInstance, rotation) as u64);
};

#[cfg(feature = "internals")]
pub(crate) mod bench;
