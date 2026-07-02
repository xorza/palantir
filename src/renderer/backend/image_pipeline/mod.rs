//! GPU side of user images. Mirrors [`crate::renderer::backend::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! are staged in [`crate::ImageRegistry`] only until upload; this module
//! drains the pending list each frame, uploads to GPU (dropping the
//! bytes), and caches the resulting bind group by registration id until
//! the owning handle drops.

mod render_target;

use crate::primitives::image::Image;
use crate::renderer::backend::Queue;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::render_target::{RenderTarget, make_target};
use crate::renderer::backend::pipeline_utils::{
    ColorVariantSpec, StencilVariant, texture_sampler_bgl,
};
use crate::renderer::gpu_view::{GPU_VIEW_FORMAT, GpuFrameCtx, GpuInitCtx};
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::{ImageInstance, RenderTargetDraw};
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

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
    /// [`TextureId`] (the registration id behind a handle).
    ///
    /// Holds bind groups for **both** registered images and `GpuView`
    /// render targets (the id authority is shared, so no collision) —
    /// `draw` is identical for both. Render-target entries are registered /
    /// freed by [`Self::paint_gpu_views`].
    cache: FxHashMap<TextureId, wgpu::BindGroup>,
    /// Framework-owned off-screen `GpuView` targets, keyed by [`TextureId`].
    /// [`Self::paint_gpu_views`] (re)allocates + paints them and frees the
    /// ones culled this frame; the bind groups live in `cache` above so the
    /// composite samples a target exactly like any image.
    gpu_view_targets: FxHashMap<TextureId, RenderTarget>,
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
            gpu_view_targets: FxHashMap::default(),
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent image objects; the per-image textures,
    /// bind groups, sampler, and layout are all format-independent.
    /// Called by `FormatPipelines` per format.
    pub(crate) fn build_variants(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        image_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Per-image tex+sampler at group 0 — viewport rides the
        // shared immediate region.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.image.pipeline",
                stencil_label: "palantir.image.pipeline.stencil_test",
                layout_label: "palantir.image.pl",
                shader,
                bind_group_layouts: &[Some(image_bgl)],
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
        images.drain_pending(|id, image| {
            let bind_group = self.upload(ctx.device, ctx.queue, id, &image);
            self.cache.insert(id, bind_group);
            // `image` (CPU bytes) dropped here — it lives only until upload.
        });
        images.drain_dropped(|id| {
            self.cache.remove(&id);
        });
    }

    /// Paint every [`GpuView`](crate::widgets::gpu_view::GpuView) drawn this
    /// frame into its off-screen target, before the main pass. Called once per
    /// frame from `WgpuBackend::submit`'s upload phase. For each `frame_targets`
    /// entry: [`Self::ensure_target`] (re)allocates the target (registering its
    /// bind group in the **shared** `cache` so the composite samples it like any
    /// image), runs [`GpuPaint::init`](crate::renderer::gpu_view::GpuPaint::init)
    /// once, then `GpuPaint::paint` into it. Never touches the instance buffer,
    /// so it only has to run before the main pass samples the targets.
    ///
    /// Eviction is **immediate**: any target absent from this frame's
    /// `frame_targets` is freed. Correct because every composited view is
    /// repainted, so a freed-then-recomposited target is never sampled blank —
    /// but a `repaint(false)` view culled from a frame frees its texture, so
    /// `GpuPaint::init` re-runs when it next composites (guard expensive setup).
    #[profiling::function]
    pub(crate) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        scale: f32,
        now: Duration,
    ) {
        for draw in frame_targets {
            let rt = self.ensure_target(ctx.device, draw.id, draw.used);
            let mut paint = draw.paint.0.borrow_mut();
            // Run `init` once per target (not on a realloc: the recreated
            // texture shares the build-time format).
            if !rt.initialized {
                profiling::scope!("GpuView::init");
                // No encoder commands recorded in `init`, so this group is
                // usually empty in a capture — kept for symmetry with paint.
                ctx.encoder.push_debug_group("palantir.gpu_view.init");
                paint.init(&GpuInitCtx {
                    device: ctx.device,
                    target_format: GPU_VIEW_FORMAT,
                });
                ctx.encoder.pop_debug_group();
                rt.initialized = true;
            }
            // Time since this view last painted (ZERO on its first paint).
            let dt = rt
                .last_paint
                .map_or(Duration::ZERO, |last| now.saturating_sub(last));
            profiling::scope!("GpuView::paint");
            // Encoder-level group so the user's own passes nest under one
            // navigable region per view in a RenderDoc / Metal capture.
            ctx.encoder.push_debug_group("palantir.gpu_view.paint");
            paint.paint(&mut GpuFrameCtx {
                device: ctx.device,
                queue: ctx.queue,
                encoder: ctx.encoder,
                target: &rt.view,
                size_px: draw.used,
                scale,
                dt,
            });
            ctx.encoder.pop_debug_group();
            rt.last_paint = Some(now);
        }
        // Evict immediately: a target absent from this frame's `frame_targets`
        // (its widget vanished, or a `repaint(false)` view was culled) is freed
        // — texture + shared-cache bind group together.
        self.gpu_view_targets.retain(|id, _| {
            let keep = frame_targets.iter().any(|draw| draw.id == *id);
            if !keep {
                self.cache.remove(id);
            }
            keep
        });
    }

    /// The off-screen target for `id`, in a single `entry` lookup. Reuses the
    /// existing texture unless the requested `size` changed; on a change (or
    /// first sight) builds a fresh texture + bind group via [`make_target`] (a
    /// realloc swaps only the texture, so `init` + last-paint state persist).
    /// `gpu_view_targets` and `image_bgl`/`sampler`/`cache` are disjoint fields,
    /// so the bind-group build borrows them alongside the held entry.
    fn ensure_target(
        &mut self,
        device: &wgpu::Device,
        id: TextureId,
        size: UVec2,
    ) -> &mut RenderTarget {
        match self.gpu_view_targets.entry(id) {
            Entry::Occupied(e) => {
                let rt = e.into_mut();
                if rt.size != size {
                    rt.view = make_target(
                        device,
                        &self.image_bgl,
                        &self.sampler,
                        &mut self.cache,
                        id,
                        size,
                    );
                    rt.size = size;
                }
                rt
            }
            Entry::Vacant(e) => e.insert(RenderTarget {
                view: make_target(
                    device,
                    &self.image_bgl,
                    &self.sampler,
                    &mut self.cache,
                    id,
                    size,
                ),
                size,
                initialized: false,
                last_paint: None,
            }),
        }
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
        id: TextureId,
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
        texture_bind_group(device, &self.image_bgl, &self.sampler, &view, &bg_label)
    }

    /// Sync the per-instance buffer — one contiguous, zero-copy upload from
    /// the shared slice; the schedule slices by batch at draw time.
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
    pub(crate) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        id: TextureId,
        instance: u32,
    ) {
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

/// Build a per-texture bind group (texture view @0 + sampler @1) against the
/// shared image layout `bgl`. One construction site for both the CPU-image
/// [`ImagePipeline::upload`] and the `GpuView` target paint
/// ([`ImagePipeline::paint_gpu_views`]), so their bindings can't drift.
fn texture_bind_group(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
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
