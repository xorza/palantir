//! GPU side of user images. Mirrors [`crate::renderer::backend::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time. The CPU texture bytes
//! are staged in [`crate::renderer::image_registry::ImageRegistry`] only until upload; this module
//! drains the pending list each frame, uploads to GPU (dropping the
//! bytes), and caches the resulting bind group by registration id until
//! the owning handle drops.

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod render_target;
mod textures;

use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_pipeline::render_target::GpuViewTargets;
use crate::renderer::backend::image_pipeline::textures::ImageTextures;
use crate::renderer::backend::instance_pipeline::InstancePipeline;
use crate::renderer::backend::shader_template::{self, ShaderConstant};
use crate::renderer::backend::stencil_variant::ColorVariantSpec;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::image::{
    FrameViews, IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TAPS_MEAN, IMG_FLAG_TAPS_PEAK,
    IMG_FLAG_TILED, ImageInstance,
};
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::text::shaper::TextShaper;
use std::time::Duration;

/// One batch of image draws: the frame's whole per-draw texture column,
/// and the slice of it this batch owns.
///
/// `items` is the same [`Span`] that indexes the per-frame instance
/// buffer, so the textures and the instances they draw cannot come from
/// different batches.
#[derive(Clone, Copy, Debug)]
pub(super) struct ImageBatch<'a> {
    pub(super) ids: &'a [TextureId],
    pub(super) items: Span,
}

#[derive(Debug)]
pub(super) struct ImagePipeline {
    instance_buffer: DynamicBuffer<ImageInstance>,
    /// Image shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,
    /// `id → bind group` for every live registration's GPU texture,
    /// together with the group 0 layout and sampler each one is built
    /// against. An entry is inserted when the registry drains a pending
    /// upload, and removed when the owning
    /// [`ImageHandle`](crate::ImageHandle) (and all its clones) drops —
    /// the registry reports those ids via `drain_dropped`. A `draw` for
    /// an absent id is skipped. Keyed by [`TextureId`] (the registration
    /// id behind a handle).
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
    /// Reconcile the GPU texture cache with the registry — see
    /// [`ImageTextures::drain_registry`].
    pub(super) fn drain_registry(&mut self, ctx: &mut GpuCtx<'_>, images: &ImageRegistry) {
        self.textures.drain_registry(ctx, images);
    }

    /// Paint this frame's [`GpuView`](crate::widgets::gpu_view::GpuView)
    /// targets and evict the submitter's dropped ones — see
    /// [`GpuViewTargets::paint`].
    pub(super) fn paint_gpu_views(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        views: FrameViews<'_>,
        owner: RenderOwnerId,
        now: Duration,
        text: &TextShaper,
    ) {
        self.gpu_view_targets
            .paint(ctx, views, owner, now, &mut self.textures, text);
    }

    /// Free every `GpuView` target owned by a retired render stream — see
    /// [`GpuViewTargets::retire_owner`]. Gated with its entry point,
    /// `WgpuBackend::retire_render_owner`, which carries the reason.
    #[cfg_attr(not(feature = "winit"), allow(dead_code))]
    pub(super) fn retire_render_owner(&mut self, owner: RenderOwnerId) {
        self.gpu_view_targets
            .retire_owner(owner, &mut self.textures);
    }
}

impl InstancePipeline for ImagePipeline {
    /// None past the device: group 0 is this pipeline's own per-image
    /// texture and sampler layout.
    type Layouts<'a> = ();
    type Upload<'a> = &'a [ImageInstance];
    /// None: group 0 is set per run inside [`Self::draw`], off the cached
    /// per-texture bind group.
    type Bindings<'a> = ();
    type Batch<'a> = ImageBatch<'a>;

    /// Format-independent image resources: the shader here, and the
    /// layout / sampler / texture cache inside [`ImageTextures`]. The
    /// pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    fn new(device: &wgpu::Device) -> Self {
        // Rust owns the flag bits; the shader declares them as markers so the
        // two cannot drift (`specialize` panics on an unsubstituted one).
        let wgsl = shader_template::specialize(
            shader_template::IMAGE_WGSL,
            &[
                ShaderConstant::uint("IMG_FLAG_TILED", IMG_FLAG_TILED),
                ShaderConstant::uint("IMG_FLAG_MIN_NEAREST", IMG_FLAG_MIN_NEAREST),
                ShaderConstant::uint("IMG_FLAG_MAG_NEAREST", IMG_FLAG_MAG_NEAREST),
                ShaderConstant::uint("IMG_FLAG_TAPS_MEAN", IMG_FLAG_TAPS_MEAN),
                ShaderConstant::uint("IMG_FLAG_TAPS_PEAK", IMG_FLAG_TAPS_PEAK),
            ],
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palantir.image.shader"),
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });

        let instance_buffer =
            DynamicBuffer::<ImageInstance>::vertex(device, "palantir.image.instances", 16);

        Self {
            instance_buffer,
            shader,
            textures: ImageTextures::new(device),
            gpu_view_targets: GpuViewTargets::default(),
        }
    }

    fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &IMAGE_INSTANCE_ATTRS,
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent image objects; the per-image textures,
    /// bind groups, sampler, and layout are all format-independent.
    /// Called by `FormatPipelines` per format.
    fn build_variants(
        &self,
        device: &wgpu::Device,
        _layouts: Self::Layouts<'_>,
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
                shader: &self.shader,
                bind_group_layouts: &[Some(&self.textures.bgl)],
                vertex_buffers: &[Some(Self::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Sync the per-instance buffer — one contiguous, zero-copy upload from
    /// the shared slice; the schedule slices by batch at draw time.
    fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: Self::Upload<'_>) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    /// Bind once per pass. Viewport rides immediates; per-image
    /// group 0 is set in [`Self::draw`] from the cached bind group.
    fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
        _bindings: Self::Bindings<'a>,
    ) {
        pass.set_pipeline(variant.select(use_stencil));
        pass.set_vertex_buffer(0, self.instance_buffer.buffer.slice(..));
    }

    /// Draw one image batch. `ids` is the frame's whole per-draw texture
    /// column and `items` selects this batch's slice of it — the same
    /// `Span` that indexes the per-frame instance buffer, so the textures
    /// and the instances they draw cannot come from different batches.
    ///
    /// Adjacent draws sharing a texture collapse into a single
    /// `set_bind_group` + instanced `draw` (see [`image_runs`]). The
    /// repeated-icon and repeated-`GpuView`-composite cases are the ones
    /// this targets; an alternating sequence yields one run per draw and
    /// records exactly what it did before.
    ///
    /// An **absent id is skipped** (no warning, no draw) — it just means
    /// the owning [`ImageHandle`](crate::ImageHandle) was dropped before
    /// this draw, or hasn't been uploaded yet. Drawing nothing is the
    /// defined behaviour for a missing texture. Every draw in a run
    /// shares one id, so the miss check runs once per run and skips the
    /// whole run, which is exactly the per-draw behaviour it replaces.
    fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        ImageBatch { ids, items }: Self::Batch<'_>,
    ) {
        for run in image_runs(&ids[items.range()], items.start) {
            let Some(bind_group) = self.textures.bindings.get(&run.id) else {
                continue;
            };
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..4, run.instances.into());
        }
    }
}

/// One maximal run of consecutive draws in a batch that share a
/// [`TextureId`], and therefore a bind group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImageRun {
    id: TextureId,
    /// Slice of the per-frame instance buffer this run draws, as one
    /// instanced range.
    instances: Span,
}

/// Split a batch's texture ids into maximal runs of *adjacent* equal ids.
///
/// Adjacency is the whole trick: only neighbours merge, so paint order is
/// preserved exactly and nothing is sorted. A non-adjacent repeat (`A B A`)
/// stays three runs — collapsing it would reorder the draws, and the
/// one-entry "last binding" cache that would spare the second `A` its hash
/// probe cannot help either, since by construction no two consecutive runs
/// share an id.
///
/// Instance indices are contiguous within a batch (the composer appends a
/// batch's rows in draw order), so each run is one `Span` and needs no
/// per-draw arithmetic. Lazy — allocates nothing.
fn image_runs(ids: &[TextureId], first_instance: u32) -> impl Iterator<Item = ImageRun> + '_ {
    let mut next = first_instance;
    ids.chunk_by(|a, b| a == b).map(move |run| {
        let instances = Span::new(next, run.len() as u32);
        next += run.len() as u32;
        ImageRun {
            id: run[0],
            instances,
        }
    })
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

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn textures(raw: &[u64]) -> Vec<TextureId> {
        raw.iter().copied().map(TextureId).collect()
    }

    /// Expand runs back into the one-draw-per-image sequence the
    /// pre-coalescing loop issued: `(instance index, texture)` in order.
    /// Equality with the input is the correctness property — same
    /// textures, same instances, same order, nothing dropped or
    /// duplicated. It also proves the spans tile the batch with no gap
    /// and no overlap, which a run-count assertion alone would not.
    fn expand(ids: &[TextureId], first_instance: u32) -> Vec<(usize, TextureId)> {
        image_runs(ids, first_instance)
            .flat_map(|run| {
                run.instances
                    .range()
                    .map(move |instance| (instance, run.id))
            })
            .collect()
    }

    #[test]
    fn adjacent_runs_coalesce_without_disturbing_draw_order() {
        // `runs` is the post-change bind + draw count: one of each per
        // run. Before coalescing every case below cost `ids.len()`.
        let cases: [(&str, &[u64], usize); 7] = [
            ("empty", &[], 0),
            ("single", &[7], 1),
            // The win: repeated icons / repeated GpuView composites.
            ("all same", &[7, 7, 7, 7], 1),
            ("adjacent groups", &[7, 7, 9, 9, 9, 4], 3),
            // Controls that must NOT shrink. Alternating is the case a
            // one-entry last-binding cache is also powerless against:
            // consecutive runs never share an id.
            ("alternating", &[7, 9, 7, 9], 4),
            ("all unique", &[1, 2, 3, 4], 4),
            // Equal ids either side of a different one stay three runs —
            // merging them would reorder paint.
            ("non-adjacent repeat", &[7, 9, 7], 3),
        ];
        for (label, raw, expected_runs) in cases {
            let ids = textures(raw);
            let runs: Vec<_> = image_runs(&ids, 0).collect();
            assert_eq!(runs.len(), expected_runs, "{label}: bind + draw count");
            let per_draw: Vec<_> = ids.iter().copied().enumerate().collect();
            assert_eq!(expand(&ids, 0), per_draw, "{label}: draw sequence");
        }
    }

    #[test]
    fn runs_index_the_batch_slice_not_the_frame() {
        // A batch starts partway into the shared instance buffer, so the
        // first run begins at `first_instance` and the rest follow it —
        // getting this wrong draws another batch's instances.
        let ids = textures(&[7, 7, 9]);
        let runs: Vec<_> = image_runs(&ids, 5).collect();
        assert_eq!(
            runs,
            [
                ImageRun {
                    id: TextureId(7),
                    instances: Span::new(5, 2),
                },
                ImageRun {
                    id: TextureId(9),
                    instances: Span::new(7, 1),
                },
            ]
        );
        assert_eq!(
            expand(&ids, 5),
            [(5, TextureId(7)), (6, TextureId(7)), (7, TextureId(9)),]
        );
    }
}
