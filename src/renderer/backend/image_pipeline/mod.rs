//! GPU side of user images. Mirrors [`crate::renderer::backend::mesh_pipeline::MeshPipeline`]
//! but draws textured quads — per-instance rect + tint, plus a
//! per-image bind group selected at draw time.
//!
//! The bind groups themselves belong to [`ImageTextures`], which the
//! backend owns beside this pipeline and hands to
//! [`ImagePipeline::draw`] — the same split the text pass makes between
//! its encoder and the atlas it fills. That store also holds the
//! `GpuView` render targets, so a composite of one binds exactly like an
//! image.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::image_textures::ImageTextures;
use crate::renderer::backend::pipeline_recipe::PipelineRecipe;
use crate::renderer::backend::shader_template::{self, ShaderConstant};
use crate::renderer::backend::stencil_variant::ColorVariantSpec;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::render_buffer::image::{
    IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TAPS_MEAN, IMG_FLAG_TAPS_PEAK,
    IMG_FLAG_TILED, ImageInstance,
};

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
}

impl ImagePipeline {
    /// Format-independent image resources: the shader and the instance
    /// buffer. The pipelines are built by
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(super) fn new(device: &wgpu::Device) -> Self {
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
        }
    }

    pub(super) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &IMAGE_INSTANCE_ATTRS,
        }
    }

    /// Build the base + stencil-test color pipelines against `format` —
    /// the only format-dependent image objects; the per-image textures,
    /// bind groups, sampler, and `image_bgl` are all format-independent.
    /// Called by `FormatPipelines` per format.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        image_bgl: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Per-image tex+sampler at group 0 — viewport rides the
        // shared immediate region.
        let layout =
            PipelineRecipe::pipeline_layout(device, "palantir.image.pl", &[Some(image_bgl)]);
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.image.pipeline",
                stencil_label: "palantir.image.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: &[Some(Self::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Sync the per-instance buffer — one contiguous, zero-copy upload from
    /// the shared slice; the schedule slices by batch at draw time.
    pub(super) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[ImageInstance]) {
        self.instance_buffer.upload_instances(ctx, instances);
    }

    /// Bind once per pass. Viewport rides immediates; per-image
    /// group 0 is set in [`Self::draw`] from the cached bind group.
    pub(super) fn bind<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        variant: &'a StencilVariant,
        use_stencil: bool,
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
    pub(super) fn draw<'a>(
        &self,
        pass: &mut wgpu::RenderPass<'a>,
        ImageBatch { ids, items }: ImageBatch<'_>,
        textures: &'a ImageTextures,
    ) {
        for run in image_runs(&ids[items.range()], items.start) {
            let Some(bind_group) = textures.bind_group(run.id) else {
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
