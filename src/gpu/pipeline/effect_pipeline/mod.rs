//! [`EffectPipeline`]: the frame's effect instances and parameter words, and the draw of each run
//! of one effect through that effect's own pipeline. The pipelines live in the
//! [`WgpuEffectStore`], which builds them as each effect loads.

use std::array;
use std::mem::offset_of;

use crate::common::span::Span;
use crate::effect::MAX_IMAGE_SLOTS;
use crate::effect::contract::{IMAGES_GROUP, PANES_GROUP};
use crate::effect::effect_kind::KindId;
use crate::effect::effect_store::EffectModule;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::effect_image_groups::{EffectImageGroups, EffectImages};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_effect_store::WgpuEffectStore;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::spectrum_tables;
use crate::primitives::identity::texture_id::TextureId;
use crate::renderer::render_buffer::effect_draw_row::{EffectInstance, EffectProgram, PaneForm};
use crate::shape::paint::lens_optics;

/// The layouts every effect's pipeline builds over: the parameter words at group 0, the images
/// at [`IMAGES_GROUP`] for an effect that binds any, a backdrop effect's pane results at
/// [`PANES_GROUP`], and the immediates every pipeline declares.
#[derive(Debug)]
pub(crate) struct EffectLayouts {
    group: wgpu::BindGroupLayout,
    /// A color effect's, at each count of image slots from 0.
    color: [wgpu::PipelineLayout; MAX_IMAGE_SLOTS as usize + 1],
    /// A backdrop effect's, at each count of image slots from 0.
    backdrop: [wgpu::PipelineLayout; MAX_IMAGE_SLOTS as usize + 1],
}

impl EffectLayouts {
    pub(crate) fn new(gpu: &Gpu, binding: &TextureBinding) -> Self {
        let group = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.effect.bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let images = |slots: usize| match u8::try_from(slots).unwrap() {
            0 => None,
            slots => Some(binding.images_layout(slots)),
        };
        let color = array::from_fn(|slots| {
            pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.effect.pl",
                &[Some(&group), images(slots)],
            )
        });
        let backdrop = array::from_fn(|slots| {
            pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.effect.backdrop.pl",
                &[Some(&group), images(slots), Some(binding.panes())],
            )
        });
        Self {
            group,
            color,
            backdrop,
        }
    }

    /// The pipeline layout of an effect of `module`'s kind and image slots.
    fn pipeline(&self, module: &EffectModule) -> &wgpu::PipelineLayout {
        let layouts = match module.kind {
            KindId::Color => &self.color,
            KindId::Backdrop => &self.backdrop,
        };
        &layouts[usize::from(module.image_slots)]
    }
}

/// `src·One + dst·Src1` on every channel: the effect's `color` plus what is under it weighted by
/// its `under`, which gives over, add, multiply and screen through one pipeline, with the coverage
/// applied exactly, as the blend is linear in it. Refused: a pipeline per blend mode (Godot's
/// render modes); source-over alone, which sends multiply and screen through a copy, as wgpu 30
/// has no framebuffer fetch (wgpu#10148).
const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::Src1,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::Src1,
        operation: wgpu::BlendOperation::Add,
    },
};

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 6] = [
    vertex_attr(
        0,
        wgpu::VertexFormat::Float32x4,
        offset_of!(EffectInstance, rect),
    ),
    vertex_attr(
        1,
        wgpu::VertexFormat::Float32x4,
        offset_of!(EffectInstance, corners),
    ),
    // `time`, `opacity`, `scale` and `rotation`, one after another.
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32x4,
        offset_of!(EffectInstance, time),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint32,
        offset_of!(EffectInstance, params),
    ),
    vertex_attr(
        4,
        wgpu::VertexFormat::Uint32,
        offset_of!(EffectInstance, pane),
    ),
    vertex_attr(
        5,
        wgpu::VertexFormat::Uint32,
        offset_of!(EffectInstance, flags),
    ),
];

/// Palantir's own backdrop effect, which a backdrop filter's pane draws through, one pipeline per
/// [`PaneForm`].
pub(crate) const PANE_SHADER: ShaderBody = ShaderBody {
    label: "palantir.effect.pane",
    wgsl: include_str!("pane.wgsl"),
    includes: &[],
    constants: PANE_CONSTANTS,
};

/// The numbers [`PANE_SHADER`] takes from the Rust types that own them.
const PANE_CONSTANTS: &[ShaderConstant] = &[
    ShaderConstant::uint("PANE_FORM_LENS", PaneForm::Lens as u32),
    ShaderConstant::uint("PANE_FORM_SPECTRUM", PaneForm::Spectrum as u32),
    ShaderConstant::uint("PANE_FLAG_WINDOW", EffectInstance::WINDOW),
    ShaderConstant::float("LINE_D", lens_optics::LINE_D),
    ShaderConstant::float("LINE_F", lens_optics::LINE_F),
    ShaderConstant::float("LINE_C", lens_optics::LINE_C),
    ShaderConstant::float("SPECTRUM_RED", lens_optics::SPECTRUM_RED),
    ShaderConstant::float("SPECTRUM_VIOLET", lens_optics::SPECTRUM_VIOLET),
    ShaderConstant::uint("SPECTRUM_SAMPLES", spectrum_tables::SPECTRUM_SAMPLES as u32),
];

/// The pane form `form` from `shader`, [`PANE_SHADER`]'s module, against `format`, with its
/// rounded-clip twin. It dithers its result before an 8-bit sRGB target stores it.
pub(crate) fn build_pane(
    gpu: &Gpu,
    layouts: &EffectLayouts,
    shader: &wgpu::ShaderModule,
    form: PaneForm,
    format: wgpu::TextureFormat,
) -> StencilVariant {
    let dither = f64::from(u8::from(
        format.is_srgb() && format.target_pixel_byte_cost() == Some(4),
    ));
    let label = format!("palantir.effect.pane.{}", form as u32);
    let recipe = PipelineRecipe {
        constants: &[
            ("DITHER_SRGB8", dither),
            ("PANE_FORM", f64::from(form as u32)),
        ],
        vertex_buffers: &[Some(INSTANCE_LAYOUT)],
        topology: wgpu::PrimitiveTopology::TriangleStrip,
        blend: Some(BLEND),
        ..PipelineRecipe::color(&label, shader, &layouts.backdrop[0], format)
    };
    let mut variant = StencilVariant::NONE;
    variant.ensure(gpu, StencilMode::Off, &recipe);
    variant.ensure(gpu, StencilMode::Test, &recipe);
    variant
}

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<EffectInstance>(),
    wgpu::VertexStepMode::Instance,
    &INSTANCE_ATTRS,
);

/// The effect at `idx`, from `module`, over the layout of its kind and image slots, against
/// `format`, with its rounded-clip twin.
pub(crate) fn build(
    gpu: &Gpu,
    layouts: &EffectLayouts,
    module: &EffectModule,
    format: wgpu::TextureFormat,
    idx: usize,
) -> StencilVariant {
    let label = format!("palantir.effect.{idx}");
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&label),
            source: wgpu::ShaderSource::Wgsl(module.text.as_str().into()),
        });
    let recipe = PipelineRecipe {
        vertex_entry: "palantir_vs",
        fragment_entry: "palantir_fs",
        vertex_buffers: &[Some(INSTANCE_LAYOUT)],
        topology: wgpu::PrimitiveTopology::TriangleStrip,
        blend: Some(BLEND),
        ..PipelineRecipe::color(&label, &shader, layouts.pipeline(module), format)
    };
    let mut variant = StencilVariant::NONE;
    variant.ensure(gpu, StencilMode::Off, &recipe);
    variant.ensure(gpu, StencilMode::Test, &recipe);
    variant
}

/// One frame's effects: their instances, their parameter words, the group the words bind
/// through, each draw's images, and the frame's format's pipelines, copied from the store as they
/// change, so a pass binds them for as long as it records.
#[derive(Debug)]
pub(crate) struct EffectPipeline {
    instances: DynamicBuffer<EffectInstance>,
    words: DynamicBuffer<u32>,
    /// Each draw's images this frame, at its index; scratch kept across frames.
    images: Vec<EffectImages>,
    /// Bound over `words`; made again when the buffer grows or shrinks.
    group: Option<wgpu::BindGroup>,
    group_capacity: usize,
    /// What `pipelines` were copied at; `None` before the first upload.
    copied: Option<PipelinesCopy>,
    pipelines: Vec<StencilVariant>,
    /// Each effect's kind, at its id, copied with `pipelines`.
    kinds: Vec<KindId>,
    /// The pane forms' pipelines for the format, at each [`PaneForm`], copied with `pipelines`.
    panes: Vec<StencilVariant>,
}

/// The format and store revision an [`EffectPipeline`]'s pipelines were copied at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PipelinesCopy {
    format: wgpu::TextureFormat,
    revision: u64,
}

/// One batch of effect draws: the frame's per-draw program column, this batch's slice of it, the
/// groups the draws bind their images through, and the group a backdrop effect reads its pane's
/// result through, `None` before the first frame with a pane.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectBatch<'a> {
    pub(crate) programs: &'a [EffectProgram],
    pub(crate) items: Span,
    pub(crate) groups: &'a EffectImageGroups,
    pub(crate) panes: Option<&'a wgpu::BindGroup>,
}

/// What one frame uploads: each draw's instance and the span of its images in `textures`, the
/// record store's image column, and the parameter words.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectUpload<'a> {
    pub(crate) instances: &'a [EffectInstance],
    pub(crate) images: &'a [Span],
    pub(crate) textures: &'a [TextureId],
    pub(crate) words: &'a [u32],
    pub(crate) format: wgpu::TextureFormat,
}

impl EffectPipeline {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            instances: DynamicBuffer::vertex(gpu, "palantir.effect.instances", 4),
            words: DynamicBuffer::storage(gpu, "palantir.effect.words", 64),
            images: Vec::new(),
            group: None,
            group_capacity: 0,
            copied: None,
            pipelines: Vec::new(),
            kinds: Vec::new(),
            panes: Vec::new(),
        }
    }

    /// Upload the frame's instances and words, note each draw's images with the image store,
    /// which makes the group of a set first drawn, and copy the format's pipelines where the
    /// store built since.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        stores: EffectStores<'_>,
        upload: EffectUpload<'_>,
    ) {
        self.instances.upload_instances(ctx, upload.instances);
        self.words.upload_instances(ctx, upload.words);
        self.images.clear();
        self.images.extend(
            upload
                .images
                .iter()
                .map(|span| EffectImages::new(&upload.textures[span.range()])),
        );
        // Neighbors share a set as a run of draws does, so each run notes its set once.
        stores.images.ensure_effect_groups(
            self.images
                .chunk_by(|a, b| a == b)
                .map(|run| run[0])
                .filter(|set| !set.ids().is_empty()),
            ctx.frame,
        );
        let store = stores.effects;
        let read = store.read();
        if self.group.is_none() || self.words.capacity() != self.group_capacity {
            self.group = Some(
                ctx.gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("palantir.effect.bg"),
                        layout: &read.attached.layouts.group,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.words.buffer.as_entire_binding(),
                        }],
                    }),
            );
            self.group_capacity = self.words.capacity();
        }
        let copy = PipelinesCopy {
            format: upload.format,
            revision: read.revision,
        };
        if self.copied != Some(copy) {
            self.pipelines.clear();
            if let Some(pipelines) = read.attached.for_format(upload.format) {
                self.pipelines.extend(pipelines.cloned());
            }
            self.kinds.clear();
            self.kinds
                .extend(read.modules.iter().map(|module| module.kind));
            self.panes.clear();
            if let Some(panes) = read.attached.panes_for_format(upload.format) {
                self.panes.extend_from_slice(panes);
            }
            self.copied = Some(copy);
        }
    }

    /// Draw one batch, one `draw` per run of instances of one effect over one set of images. A
    /// run whose image is not on the device is skipped, as an image draw is.
    pub(crate) fn draw_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        stencil: StencilMode,
        EffectBatch {
            programs,
            items,
            groups,
            panes,
        }: EffectBatch<'_>,
    ) {
        let group = self
            .group
            .as_ref()
            .expect("a frame uploads before it draws");
        rec.vertex(0, &self.instances.buffer);
        for run in Runs::new(items, |i| (programs[i as usize], self.images[i as usize])) {
            let (program, images) = run.key;
            let images_group = if images.ids().is_empty() {
                None
            } else {
                let Some(images_group) = groups.get(&images) else {
                    continue;
                };
                Some(images_group)
            };
            let (pipeline, kind) = match program {
                EffectProgram::App(effect) => {
                    (&self.pipelines[effect.idx()], self.kinds[effect.idx()])
                }
                EffectProgram::Pane(form) => (&self.panes[form.idx()], KindId::Backdrop),
            };
            rec.pipeline(pipeline.select(stencil));
            rec.group(0, group);
            if let Some(images_group) = images_group {
                rec.group_once(IMAGES_GROUP, images_group);
            }
            if kind == KindId::Backdrop {
                let panes = panes.expect("a frame with a backdrop effect prepares its panes");
                rec.group_once(PANES_GROUP, panes);
            }
            rec.draw(0..4, run.instances.u32_range());
        }
    }
}

/// The two stores an effect upload reads: the effects' pipelines and the images' textures.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectStores<'a> {
    pub(crate) effects: &'a WgpuEffectStore,
    pub(crate) images: &'a WgpuImageStore,
}
