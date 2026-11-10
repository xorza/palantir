//! GPU side of user images: like [`crate::gpu::pipeline::mesh_pipeline::MeshPipeline`]
//! but draws textured quads (per-instance rect + tint, per-image bind group).
//!
//! Bind groups belong to the two stores the backend hands
//! [`ImagePipeline::draw_batch`]: [`WgpuImageStore`](crate::gpu::resource::wgpu_image_store::WgpuImageStore)
//! and [`GpuViewTargets`]. Both build against one binding shape, so a view
//! composite binds like an image.

use crate::common::span::Span;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::clip_variant::ClipVariant;
use crate::gpu::pipeline::instanced_pipeline::{
    InstancedPipeline, InstancedSpec, VariantRecipe, VariantTarget,
};
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::pipeline_recipe::{self, Pipeline, vertex_attr};
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::pipeline::shape_template;
use crate::gpu::resource::gpu_view_targets::GpuViewTargets;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::ImageTexture;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::wgsl;
use crate::renderer::render_buffer::image_draw_row::ImageInstance;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::mem::offset_of;

/// One batch of image draws: the frame's per-draw texture and form columns and this
/// batch's slice of them. `items` is the same [`Span`] that indexes the instance
/// buffer, so textures and instances can't come from different batches.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageBatch<'a> {
    pub(crate) ids: &'a [TextureId],
    pub(crate) forms: &'a [ShapeForm],
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
    family: InstancedPipeline<ImageInstance>,
}

impl ImagePipeline {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.image.shader",
        wgsl: include_str!("shader.wgsl"),
        includes: &[
            wgsl::FRAME,
            include_str!("paint.wgsl"),
            wgsl::SHAPE_TEMPLATE,
            include_str!("shaped.wgsl"),
        ],
        constants: SHADER_CONSTANTS,
    };

    /// Format-independent resources: shader and instance buffer. Pipelines come
    /// from [`Self::ensure`] via
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(gpu: &Gpu, textures: &TextureBinding) -> Self {
        Self {
            family: InstancedPipeline::new(
                gpu,
                InstancedSpec {
                    shader: Self::SHADER,
                    layout: "palantir.image.pl",
                    // The frame group, then the image's texture.
                    groups: &[Some(textures.frame()), Some(textures.layout())],
                    instances: "palantir.image.instances",
                    capacity: 16,
                },
            ),
        }
    }

    /// Build the half of `form`'s color pipeline `target` binds, the only format-dependent image object. Called by `FormatPipelines` per format.
    pub(super) fn ensure(
        &self,
        variant: &mut ClipVariant,
        gpu: &Gpu,
        target: VariantTarget,
        form: ShapeForm,
    ) {
        let recipe = match form {
            ShapeForm::Rect => VariantRecipe {
                label: "palantir.image.pipeline",
                vertex_entry: "vs",
                fragment_entry: "fs",
                vertex_buffers: &[Some(INSTANCE_LAYOUT)],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            ShapeForm::Box => VariantRecipe {
                label: "palantir.image.pipeline.box",
                vertex_entry: "vs_box",
                fragment_entry: "fs_shaped",
                vertex_buffers: &[Some(INSTANCE_LAYOUT)],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            ShapeForm::Triangle => VariantRecipe {
                label: "palantir.image.pipeline.triangle",
                vertex_entry: "vs_triangle",
                fragment_entry: "fs_shaped",
                vertex_buffers: &[Some(INSTANCE_LAYOUT)],
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
        };
        self.family.ensure(variant, gpu, target, recipe);
    }

    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, instances: &[ImageInstance]) {
        self.family.upload(ctx, instances);
    }

    /// Draw one image batch, each draw through its form's pipeline of `pipelines`. `ids` is the
    /// frame's per-draw texture column and `items` selects this batch's slice (see
    /// [`ImageBatch`]).
    ///
    /// Adjacent draws sharing a form and a texture collapse into one `set_bind_group` +
    /// instanced `draw` (see [`Runs`]).
    ///
    /// An **absent id is skipped** silently: its owning
    /// [`ImageHandle`](crate::ImageHandle) was dropped. The check runs once per run.
    pub(crate) fn draw_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        pipelines: impl Fn(ShapeForm) -> &'a Pipeline,
        ImageBatch { ids, forms, items }: ImageBatch<'_>,
        textures: ImageTextures<'_>,
    ) {
        rec.vertex(0, self.family.instances());
        let key = |i: u32| (forms[i as usize], ids[i as usize]);
        for run in Runs::new(items, key) {
            let (form, id) = run.key;
            let Some(bind_group) = textures.bind_group(id) else {
                continue;
            };
            rec.pipeline(pipelines(form));
            rec.group_once(IMAGE_GROUP, bind_group);
            rec.draw(0..shape_template::vertices(form), run.instances.u32_range());
        }
    }
}

/// The group an image's texture binds at, past the frame group.
const IMAGE_GROUP: u32 = 1;

/// The image's header, then its uv crop `(min, size)`, its tint (linear straight-alpha, which the
/// shader premultiplies at write), its flags and its decal `(min, max)`.
const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 10] = {
    let h = shape_template::HEADER_ATTRS;
    let at = shape_template::HEADER_LOCATIONS;
    [
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        vertex_attr(
            at,
            wgpu::VertexFormat::Float32x4,
            offset_of!(ImageInstance, uv_min),
        ),
        vertex_attr(
            at + 1,
            wgpu::VertexFormat::Float16x4,
            offset_of!(ImageInstance, tint),
        ),
        vertex_attr(
            at + 2,
            wgpu::VertexFormat::Uint32,
            offset_of!(ImageInstance, flags),
        ),
        vertex_attr(
            at + 3,
            wgpu::VertexFormat::Float32x4,
            offset_of!(ImageInstance, decal_min),
        ),
    ]
};

const _: () = {
    assert!(offset_of!(ImageInstance, header) == 0);
    assert!(
        offset_of!(ImageInstance, uv_size) == offset_of!(ImageInstance, uv_min) + size_of::<Vec2>()
    );
    assert!(
        offset_of!(ImageInstance, decal_max)
            == offset_of!(ImageInstance, decal_min) + size_of::<Vec2>()
    );
};

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<ImageInstance>(),
    wgpu::VertexStepMode::Instance,
    &IMAGE_INSTANCE_ATTRS,
);

/// The numbers [`ImagePipeline::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("IMAGE_GROUP", IMAGE_GROUP),
    ShaderConstant::uint("IMG_FLAG_TILED", ImageFlags::TILED.bits()),
    ShaderConstant::uint("IMG_FLAG_MIN_NEAREST", ImageFlags::MIN_NEAREST.bits()),
    ShaderConstant::uint("IMG_FLAG_MAG_NEAREST", ImageFlags::MAG_NEAREST.bits()),
    ShaderConstant::uint("IMG_FLAG_TAPS_MEAN", ImageFlags::TAPS_MEAN.bits()),
    ShaderConstant::uint("IMG_FLAG_TAPS_PEAK", ImageFlags::TAPS_PEAK.bits()),
    ShaderConstant::uint("IMG_FLAG_CLAMP_TO_UV", ImageFlags::CLAMP_TO_UV.bits()),
    shape_template::CONSTANT,
];

#[cfg(feature = "internals")]
pub(crate) mod bench;
