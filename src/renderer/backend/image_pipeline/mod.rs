//! GPU side of user images. Mirrors [`crate::renderer::backend::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! are staged in [`crate::renderer::image_registry::ImageRegistry`] only until upload; this module
//! drains the pending list each frame, uploads to GPU (dropping the
//! bytes), and caches the resulting bind group by registration id until
//! the owning handle drops.

mod render_target;
mod textures;

use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::render_target::GpuViewTargets;
use crate::renderer::backend::image_pipeline::textures::ImageTextures;
use crate::renderer::backend::pipeline_utils::{
    ColorVariantSpec, StencilVariant, texture_sampler_bgl,
};
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::image::{ImageInstance, RenderTargetDraw};
use crate::renderer::render_owner::RenderOwnerId;
use crate::renderer::texture_id::TextureId;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct ImagePipeline {
    instance_buffer: DynamicBuffer<ImageInstance>,
    /// Image shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
    /// Group 0 layout (per-image texture + sampler). Built once;
    /// every cached bind group references it, and
    /// [`Self::build_variants`] composes each format's pipeline layout
    /// against it.
    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// `id → bind group` for every live registration's GPU texture. An
    /// entry is inserted when the registry drains a pending upload, and
    /// removed when the owning [`ImageHandle`](crate::ImageHandle) (and
    /// all its clones) drops — the registry reports those ids via
    /// `drain_dropped`. A `draw` for an absent id is skipped. Keyed by
    /// [`TextureId`] (the registration id behind a handle).
    ///
    /// Holds bind groups for **both** registered images and `GpuView`
    /// render targets (the id authority is shared, so no collision) —
    /// `draw` is identical for both. Render-target entries are registered /
    /// freed by [`Self::paint_gpu_views`].
    textures: ImageTextures,
    /// Framework-owned off-screen `GpuView` targets, keyed by [`TextureId`].
    /// [`Self::paint_gpu_views`] (re)allocates + paints them and frees the
    /// submitting window's culled ones. Its bind groups live in the shared
    /// texture-binding store above, so composites sample targets like images.
    gpu_view_targets: GpuViewTargets,
}

impl ImagePipeline {
    /// Format-independent image resources (shader, layout, sampler, GPU
    /// texture cache). The pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variant`].
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aperture.image.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });

        let image_bgl = texture_sampler_bgl(device, "aperture.image.tex.bgl");

        // Min/mag nearest filtering is a shader-side UV texel-center snap,
        // keeping all filter combinations on one sampler and bind group.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aperture.image.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let instance_buffer =
            DynamicBuffer::<ImageInstance>::vertex(device, "aperture.image.instances", 16);

        Self {
            instance_buffer,
            shader,
            image_bgl,
            sampler,
            textures: ImageTextures::default(),
            gpu_view_targets: GpuViewTargets::default(),
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent image objects; the per-image textures,
    /// bind groups, sampler, and layout are all format-independent.
    /// Called by `FormatPipelines` per format.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Per-image tex+sampler at group 0 — viewport rides the
        // shared immediate region.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "aperture.image.pipeline",
                stencil_label: "aperture.image.pipeline.stencil_test",
                layout_label: "aperture.image.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(&self.image_bgl)],
                vertex_buffers: &[Some(instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Reconcile the GPU texture cache with the registry, once per frame
    /// from `WgpuBackend::submit` before the render pass. Uploads newly
    /// registered images (dropping each `Image` right after upload, so the
    /// CPU bytes don't outlive the GPU copy), then frees textures whose
    /// owning [`ImageHandle`](crate::ImageHandle) dropped. After this,
    /// every still-owned image has a bind group in the cache; a draw for
    /// any other id is silently skipped.
    ///
    /// Uploads run *before* drop-frees so an image registered and dropped
    /// in the same frame uploads then frees (no orphan) rather than
    /// free-then-upload (which would leak it into the cache un-owned).
    #[profiling::function]
    pub(crate) fn drain_registry(&mut self, ctx: &mut GpuCtx<'_>, images: &ImageRegistry) {
        self.textures
            .drain_registry(ctx, images, &self.image_bgl, &self.sampler);
    }

    /// Paint every [`GpuView`](crate::widgets::gpu_view::GpuView) drawn this
    /// frame into its off-screen target, before the main pass. Called once per
    /// frame from `WgpuBackend::submit`'s upload phase. The target store
    /// allocates or resizes each entry, registers its bind group in the shared
    /// image-texture store, and runs [`GpuPaint::init`](crate::renderer::gpu_view::GpuPaint::init)
    /// once, then `GpuPaint::paint` into it. Never touches the instance buffer,
    /// so it only has to run before the main pass samples the targets.
    ///
    /// Eviction is **immediate but owner-scoped**: any target `owner`
    /// painted before that is absent from this `frame_targets` is freed.
    /// Correct because every composited view is repainted, so a
    /// freed-then-recomposited target is never sampled blank — but a
    /// `repaint(false)` view culled from a frame frees its texture, so
    /// `GpuPaint::init` re-runs when it next composites (guard expensive
    /// setup). `owner` is the submitting window's stable render-stream
    /// identity: the one shared backend serves all windows, so a submit may only
    /// evict its *own* absent targets — another window's targets survive
    /// both this submit and their owner's idle (non-submitting) frames.
    #[profiling::function]
    pub(crate) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        owner: RenderOwnerId,
        now: Duration,
    ) {
        self.gpu_view_targets.paint(
            ctx,
            frame_targets,
            owner,
            now,
            &mut self.textures,
            &self.image_bgl,
            &self.sampler,
        );
    }

    /// Sync the per-instance buffer — one contiguous, zero-copy upload from
    /// the shared slice; the schedule slices by batch at draw time.
    #[profiling::function]
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, instances: &[ImageInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
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
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        id: TextureId,
        instance: u32,
    ) {
        let Some(bind_group) = self.textures.bindings.get(&id) else {
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
    5 => Uint32,    // flags (IMG_FLAG_* bits: tile wrap, nearest)
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
    assert!(IMAGE_INSTANCE_ATTRS[5].offset == offset_of!(ImageInstance, flags) as u64);
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
            self.textures.bindings.len()
        }
    }
}
