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
    ColorVariantSpec, StencilVariant, texture_bind_group, texture_sampler_bgl,
};
use crate::renderer::gpu_view::{GPU_VIEW_FORMAT, GpuFrameCtx, GpuInitCtx};
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::{ImageInstance, RenderTargetDraw};
use crate::renderer::texture_id::TextureId;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct ImagePipeline {
    instance_buffer: DynamicBuffer,
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
    cache: FxHashMap<TextureId, wgpu::BindGroup>,
    /// Framework-owned off-screen `GpuView` targets, keyed by [`TextureId`].
    /// [`Self::paint_gpu_views`] (re)allocates + paints them and frees the
    /// submitting window's culled ones (eviction is owner-scoped — see
    /// [`keep_target`]); the bind groups live in `cache` above so the
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
            label: Some("aperture.image.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image.wgsl").into()),
        });

        let image_bgl = texture_sampler_bgl(device, "aperture.image.tex.bgl");

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
            DynamicBuffer::vertex::<ImageInstance>(device, "aperture.image.instances", 16);

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
    /// Eviction is **immediate but owner-scoped**: any target `owner`
    /// painted before that is absent from this `frame_targets` is freed.
    /// Correct because every composited view is repainted, so a
    /// freed-then-recomposited target is never sampled blank — but a
    /// `repaint(false)` view culled from a frame frees its texture, so
    /// `GpuPaint::init` re-runs when it next composites (guard expensive
    /// setup). `owner` is the submitting window's stable buffer identity
    /// ([`RenderBuffer::owner`](crate::renderer::render_buffer::RenderBuffer)):
    /// the one shared backend serves all windows, so a submit may only
    /// evict its *own* absent targets — another window's targets survive
    /// both this submit and their owner's idle (non-submitting) frames.
    #[profiling::function]
    pub(crate) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame_targets: &[RenderTargetDraw],
        owner: u64,
        scale: f32,
        now: Duration,
    ) {
        for draw in frame_targets {
            let rt = self.ensure_target(ctx.device, draw.id, draw.used, owner);
            let mut paint = draw.paint.0.borrow_mut();
            // Run `init` once per target (not on a realloc: the recreated
            // texture shares the build-time format).
            if !rt.initialized {
                profiling::scope!("GpuView::init");
                // No encoder commands recorded in `init`, so this group is
                // usually empty in a capture — kept for symmetry with paint.
                ctx.encoder.push_debug_group("aperture.gpu_view.init");
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
            ctx.encoder.push_debug_group("aperture.gpu_view.paint");
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
        // Evict immediately, owner-scoped: a target of *this* submitter absent
        // from this frame's `frame_targets` (its widget vanished, or a
        // `repaint(false)` view was culled) is freed — texture + shared-cache
        // bind group together. Other windows' targets are left alone.
        self.gpu_view_targets.retain(|id, rt| {
            let keep = keep_target(rt.owner, *id, owner, frame_targets);
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
        owner: u64,
    ) -> &mut RenderTarget {
        match self.gpu_view_targets.entry(id) {
            Entry::Occupied(e) => {
                let rt = e.into_mut();
                rt.owner = owner;
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
                owner,
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
        // First point on the upload path that knows the device limit —
        // fail here with the image's size instead of a generic wgpu
        // validation panic inside `create_texture`.
        assert_image_uploadable(
            image.width,
            image.height,
            device.limits().max_texture_dimension_2d,
        );
        let id = id.0;
        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        // Per-handle labels surface in wgpu validation traces so a
        // mis-bound image points to its source asset directly.
        let tex_label = format!("aperture.image.tex.{id:016x}");
        let bg_label = format!("aperture.image.tex.bg.{id:016x}");
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

/// Whether a `GpuView` target entry survives a submit — the
/// [`ImagePipeline::paint_gpu_views`] eviction policy. Evict only when the
/// submitting `owner` painted the entry *and* left it out of its
/// `frame_targets`; entries owned by other windows are never this submit's
/// to free (an idle window isn't submitting, so its targets must outlive
/// every other window's frames).
fn keep_target(
    entry_owner: u64,
    id: TextureId,
    owner: u64,
    frame_targets: &[RenderTargetDraw],
) -> bool {
    entry_owner != owner || frame_targets.iter().any(|draw| draw.id == id)
}

/// Guard the image-upload boundary: zero-sized or over-device-limit
/// dimensions would otherwise surface as a generic wgpu validation panic
/// inside `create_texture`, a frame after the bad `register_image` call.
/// User data (a decoded file can legitimately exceed the device limit),
/// but the registry can't see the limit — so the earliest point that can
/// check is this upload, and the failure names the actionable facts.
fn assert_image_uploadable(width: u32, height: u32, max_dim: u32) {
    assert!(
        width > 0 && height > 0,
        "registered image has zero dimension ({width}x{height})",
    );
    assert!(
        width <= max_dim && height <= max_dim,
        "registered image is {width}x{height} px but the device's \
         max_texture_dimension_2d is {max_dim}; downscale or tile it \
         before Ui::register_image",
    );
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::image_pipeline::*;
    use crate::renderer::gpu_view::{GpuFrameCtx, GpuPaint, GpuPaintRef};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug)]
    struct NoopPaint;
    impl GpuPaint for NoopPaint {
        fn paint(&mut self, _ctx: &mut GpuFrameCtx<'_>) {}
    }

    fn draw(id: u64) -> RenderTargetDraw {
        RenderTargetDraw {
            id: TextureId(id),
            used: UVec2::ONE,
            paint: GpuPaintRef(Rc::new(RefCell::new(NoopPaint))),
        }
    }

    /// Apply [`keep_target`] over `entries` (`(texture id, owner)` pairs)
    /// for one submit — returns the evicted texture ids, mirroring the
    /// `paint_gpu_views` retain.
    fn evicted(entries: &[(u64, u64)], owner: u64, frame_targets: &[RenderTargetDraw]) -> Vec<u64> {
        entries
            .iter()
            .filter(|(id, entry_owner)| {
                !keep_target(*entry_owner, TextureId(*id), owner, frame_targets)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// The owner-scoped eviction policy, table-driven. Window A owns
    /// targets 1 and 3, window B owns target 2. Only the submitter's own
    /// absent targets are evicted — an idle window's targets survive both
    /// other windows' submits and its own non-submitting frames.
    #[test]
    fn gpu_view_eviction_is_owner_scoped() {
        const A: u64 = 10;
        const B: u64 = 20;
        let entries = [(1, A), (3, A), (2, B)];
        #[derive(Debug)]
        struct Case {
            name: &'static str,
            owner: u64,
            frame: Vec<RenderTargetDraw>,
            expect_evicted: Vec<u64>,
        }
        let cases = [
            Case {
                // A composits both its views; B's idle target untouched.
                name: "A submits all its targets",
                owner: A,
                frame: vec![draw(1), draw(3)],
                expect_evicted: vec![],
            },
            Case {
                // A culled view 3 (repaint(false) or widget gone) — only
                // A's absent target goes; B's stays.
                name: "A submits without target 3",
                owner: A,
                frame: vec![draw(1)],
                expect_evicted: vec![3],
            },
            Case {
                // B's submit lists only its own view — A's targets must
                // survive even though they're absent from B's frame.
                name: "B submits its target",
                owner: B,
                frame: vec![draw(2)],
                expect_evicted: vec![],
            },
            Case {
                // B stops compositing its view: exactly its own target is
                // freed, never the other window's.
                name: "B submits empty frame",
                owner: B,
                frame: vec![],
                expect_evicted: vec![2],
            },
            Case {
                // A's GpuViews all vanish: both its targets are freed in
                // one submit; B's survives.
                name: "A submits empty frame",
                owner: A,
                frame: vec![],
                expect_evicted: vec![1, 3],
            },
        ];
        for case in cases {
            assert_eq!(
                evicted(&entries, case.owner, &case.frame),
                case.expect_evicted,
                "case: {}",
                case.name,
            );
        }
    }

    #[test]
    fn image_within_device_limit_is_uploadable() {
        // Boundary: exactly the limit passes, 1x1 passes.
        assert_image_uploadable(1, 1, 8192);
        assert_image_uploadable(8192, 8192, 8192);
    }

    #[test]
    #[should_panic(expected = "max_texture_dimension_2d is 8192")]
    fn oversized_image_panics_with_named_limit() {
        assert_image_uploadable(8193, 4, 8192);
    }

    #[test]
    #[should_panic(expected = "zero dimension")]
    fn zero_dim_image_panics_at_upload_boundary() {
        assert_image_uploadable(0, 4, 8192);
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
