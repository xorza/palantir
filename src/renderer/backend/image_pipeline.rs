//! GPU side of user images. Mirrors [`crate::renderer::backend::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! are staged in [`crate::ImageRegistry`] only until upload; this module
//! drains the pending list each frame, uploads to GPU (dropping the
//! bytes), and caches the resulting bind group by registration id until
//! the owning handle drops.

use crate::primitives::image::Image;
use crate::renderer::backend::Queue;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{
    PipelineRecipe, StencilVariant, build_pipeline, build_pipeline_layout, texture_sampler_bgl,
};
use crate::renderer::backend::stencil::stencil_test_state;
use crate::renderer::image_registry::{ImageId, ImageRegistry};
use crate::renderer::render_buffer::ImageInstance;
use rustc_hash::FxHashMap;

pub(crate) struct ImagePipeline {
    instance_buffer: DynamicBuffer,
    /// Image shader module — format-independent; `FormatPipelines` reads
    /// it to build this format's pipelines.
    pub(crate) shader: wgpu::ShaderModule,
    /// Group 0 layout (per-image texture + sampler). Built once;
    /// every cached bind group references it. Format-independent;
    /// `FormatPipelines` reads it to compose the pipeline layout.
    pub(crate) image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// `id → bind group` for every live registration's GPU texture. An
    /// entry is inserted when the registry drains a pending upload, and
    /// removed when the owning [`ImageHandle`](crate::ImageHandle) (and
    /// all its clones) drops — the registry reports those ids via
    /// `drain_dropped`. A `draw` for an absent id is skipped. Keyed by
    /// [`ImageId`] (the registration id behind a handle).
    cache: FxHashMap<ImageId, wgpu::BindGroup>,
}

impl ImagePipeline {
    /// Format-independent image resources (shader, layout, sampler, GPU
    /// texture cache). The pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.image.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });

        let image_bgl = texture_sampler_bgl(device, "palantir.image.tex.bgl");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.image.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let instance_buffer =
            DynamicBuffer::vertex::<ImageInstance>(device, "palantir.image.instances", 16);

        Self {
            instance_buffer,
            shader,
            image_bgl,
            sampler,
            cache: FxHashMap::default(),
        }
    }

    /// Build the color pipeline against `format`. The only
    /// format-dependent object in the whole pipeline — the per-image
    /// textures, bind groups, sampler, and layout are all
    /// format-independent. `stencil` selects the rounded-clip variant
    /// (adds the shared `stencil_test_state`). Called by `FormatPipelines`
    /// per format.
    pub(crate) fn build_variant(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        image_bgl: &wgpu::BindGroupLayout,
        color_format: wgpu::TextureFormat,
        stencil: bool,
    ) -> wgpu::RenderPipeline {
        let (label, layout_label, depth_stencil) = if stencil {
            (
                "palantir.image.pipeline.stencil_test",
                "palantir.image.pl.stencil",
                Some(stencil_test_state()),
            )
        } else {
            ("palantir.image.pipeline", "palantir.image.pl", None)
        };
        // Per-image tex+sampler at group 0 — viewport rides the
        // shared immediate region.
        let layout = build_pipeline_layout(device, layout_label, &[Some(image_bgl)]);
        build_pipeline(
            device,
            PipelineRecipe {
                label,
                shader,
                layout: &layout,
                vertex_buffers: &[instance_layout()],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil,
            },
        )
    }

    /// Reconcile the GPU texture cache with the registry, once per frame
    /// from `WgpuBackend::submit` before the render pass. Frees textures
    /// whose owning [`ImageHandle`](crate::ImageHandle) dropped, then
    /// uploads newly registered images (dropping each `Image` right after
    /// upload, so the CPU bytes don't outlive the GPU copy). After this,
    /// every still-owned image has a bind group in the cache; a draw for
    /// any other id is silently skipped.
    ///
    /// Uploads run *before* drop-frees so an image registered and dropped
    /// in the same frame uploads then frees (no orphan) rather than
    /// free-then-upload (which would leak it into the cache un-owned).
    #[profiling::function]
    pub(crate) fn drain_registry(&mut self, ctx: &mut GpuCtx<'_>, images: &ImageRegistry) {
        images.drain_pending(|id, image| {
            let bind_group = self.upload(ctx.device, ctx.queue, id, &image);
            self.cache.insert(id, bind_group);
            // `image` (CPU bytes) dropped here — it lives only until upload.
        });
        images.drain_dropped(|id| {
            self.cache.remove(&id);
        });
    }

    /// Upload a fresh RGBA8 texture for `id` and build its per-image
    /// bind group. The texture + view are held only by the returned
    /// `BindGroup` — wgpu's internal Arcs keep them alive for the
    /// bind group's lifetime; dropping the wrapper frees the GPU
    /// resources too.
    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &Queue,
        id: ImageId,
        image: &Image,
    ) -> wgpu::BindGroup {
        let id = id.0;
        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        // Per-handle labels surface in wgpu validation traces so a
        // mis-bound image points to its source asset directly.
        let tex_label = format!("palantir.image.tex.{id:016x}");
        let bg_label = format!("palantir.image.tex.bg.{id:016x}");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&tex_label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB format: sampler decodes to linear automatically.
            // CPU bytes are sRGB-encoded straight-alpha RGBA8.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            size,
        );
        let view = texture.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&bg_label),
            layout: &self.image_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Sync the per-instance buffer. Single contiguous upload — the
    /// schedule slices by batch at draw time.
    #[profiling::function]
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, instances: &[ImageInstance]) {
        if instances.is_empty() {
            return;
        }
        self.instance_buffer
            .upload(ctx, bytemuck::cast_slice(instances), instances.len());
    }

    /// Bind once per pass. Viewport rides immediates; per-image
    /// group 0 is set in [`Self::draw`] from the cached bind group.
    pub(crate) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
    ) {
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_vertex_buffer(0, self.instance_buffer.buffer.slice(..));
    }

    /// Issue one image draw. `instance` indexes into the per-frame
    /// instance buffer. `id` selects the bind group; an **absent id is
    /// skipped** (no warning, no draw) — it just means the owning
    /// [`ImageHandle`](crate::ImageHandle) was dropped before this draw,
    /// or hasn't been uploaded yet. Drawing nothing is the defined
    /// behaviour for a missing texture.
    pub(crate) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, id: ImageId, instance: u32) {
        let Some(bind_group) = self.cache.get(&id) else {
            return;
        };
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..4, instance..instance + 1);
    }
}

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, // rect.min
    1 => Float32x2, // rect.size
    2 => Float32x2, // uv_min
    3 => Float32x2, // uv_size
    // `Unorm8x4` normalizes `u8/255 → 0..1`. Tint is linear straight-alpha
    // on the CPU; shader multiplies by the sampled texel and premultiplies
    // at write.
    4 => Unorm8x4,  // tint
    5 => Uint32,    // tiled (1 = fract-wrap UV)
];

// Compile-time guard: attribute offsets must match the `ImageInstance`
// fields they feed. `array_stride == size_of` alone wouldn't catch a
// same-size field reorder or a format/field size mismatch; `offset_of!`
// does.
const _: () = {
    use std::mem::offset_of;
    assert!(IMAGE_INSTANCE_ATTRS[0].offset == offset_of!(ImageInstance, rect.min) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[1].offset == offset_of!(ImageInstance, rect.size) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[2].offset == offset_of!(ImageInstance, uv_min) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[3].offset == offset_of!(ImageInstance, uv_size) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[4].offset == offset_of!(ImageInstance, tint) as u64);
    assert!(IMAGE_INSTANCE_ATTRS[5].offset == offset_of!(ImageInstance, tiled) as u64);
};

fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ImageInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &IMAGE_INSTANCE_ATTRS,
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    //! Reach-in for the surface-format-change tests: GPU texture-cache
    //! occupancy, used to assert the cache survives a pipeline rebuild.

    use crate::renderer::backend::image_pipeline::*;

    impl ImagePipeline {
        /// Count of images currently resident in the GPU texture cache.
        /// Lets the surface-format-change tests assert the cache survives
        /// a pipeline rebuild (surgical rebuild keeps it; a full rebuild
        /// would drop it to zero).
        pub(crate) fn gpu_cached_count(&self) -> usize {
            self.cache.len()
        }
    }
}
