//! [`EffectPipeline`]: the frame's effect instances and parameter words, and the draw of each run
//! of one effect through that effect's own pipeline. The pipelines live in the
//! [`WgpuEffectStore`], which builds them as each effect loads.

use std::mem::offset_of;

use crate::common::span::Span;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::runs::Runs;
use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::wgpu_effect_store::WgpuEffectStore;
use crate::primitives::identity::effect_id::EffectId;
use crate::renderer::render_buffer::effect_draw_row::EffectInstance;

/// The layouts every effect's pipeline builds over: the parameter words at group 0, and the
/// immediates every pipeline declares.
#[derive(Debug)]
pub(crate) struct EffectLayouts {
    group: wgpu::BindGroupLayout,
    pipeline: wgpu::PipelineLayout,
}

impl EffectLayouts {
    pub(crate) fn new(gpu: &Gpu) -> Self {
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
        let pipeline = pipeline_recipe::pipeline_layout(gpu, "palantir.effect.pl", &[Some(&group)]);
        Self { group, pipeline }
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

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = [
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
    // `time`, `opacity` and `scale`, one after another.
    vertex_attr(
        2,
        wgpu::VertexFormat::Float32x3,
        offset_of!(EffectInstance, time),
    ),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint32,
        offset_of!(EffectInstance, params),
    ),
];

const INSTANCE_LAYOUT: wgpu::VertexBufferLayout<'static> = pipeline_recipe::vertex_layout(
    size_of::<EffectInstance>(),
    wgpu::VertexStepMode::Instance,
    &INSTANCE_ATTRS,
);

/// The effect at `idx`, from its module `text`, against `format`, with its rounded-clip twin.
pub(crate) fn build(
    gpu: &Gpu,
    layouts: &EffectLayouts,
    text: &str,
    format: wgpu::TextureFormat,
    idx: usize,
) -> StencilVariant {
    let label = format!("palantir.effect.{idx}");
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&label),
            source: wgpu::ShaderSource::Wgsl(text.into()),
        });
    let recipe = PipelineRecipe {
        vertex_entry: "palantir_vs",
        fragment_entry: "palantir_fs",
        vertex_buffers: &[Some(INSTANCE_LAYOUT)],
        topology: wgpu::PrimitiveTopology::TriangleStrip,
        blend: Some(BLEND),
        ..PipelineRecipe::color(&label, &shader, &layouts.pipeline, format)
    };
    let mut variant = StencilVariant::NONE;
    variant.ensure(gpu, StencilMode::Off, &recipe);
    variant.ensure(gpu, StencilMode::Test, &recipe);
    variant
}

/// One frame's effects: their instances, their parameter words, the group the words bind
/// through, and the frame's format's pipelines, copied from the store as they change, so a pass
/// binds them for as long as it records.
#[derive(Debug)]
pub(crate) struct EffectPipeline {
    instances: DynamicBuffer<EffectInstance>,
    words: DynamicBuffer<u32>,
    /// Bound over `words`; made again when the buffer grows or shrinks.
    group: Option<wgpu::BindGroup>,
    group_capacity: usize,
    /// What `pipelines` were copied at; `None` before the first upload.
    copied: Option<PipelinesCopy>,
    pipelines: Vec<StencilVariant>,
}

/// The format and store revision an [`EffectPipeline`]'s pipelines were copied at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PipelinesCopy {
    format: wgpu::TextureFormat,
    revision: u64,
}

/// One batch of effect draws: the frame's per-draw effect column and this batch's slice of it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectBatch<'a> {
    pub(crate) ids: &'a [EffectId],
    pub(crate) items: Span,
}

/// What one frame uploads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EffectUpload<'a> {
    pub(crate) instances: &'a [EffectInstance],
    pub(crate) words: &'a [u32],
    pub(crate) format: wgpu::TextureFormat,
}

impl EffectPipeline {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            instances: DynamicBuffer::vertex(gpu, "palantir.effect.instances", 4),
            words: DynamicBuffer::storage(gpu, "palantir.effect.words", 64),
            group: None,
            group_capacity: 0,
            copied: None,
            pipelines: Vec::new(),
        }
    }

    /// Upload the frame's instances and words, and copy the format's pipelines where the store
    /// built since.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        store: &WgpuEffectStore,
        upload: EffectUpload<'_>,
    ) {
        self.instances.upload_instances(ctx, upload.instances);
        self.words.upload_instances(ctx, upload.words);
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
            self.copied = Some(copy);
        }
    }

    /// Draw one batch, one `draw` per run of instances of one effect.
    pub(crate) fn draw_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        stencil: StencilMode,
        EffectBatch { ids, items }: EffectBatch<'_>,
    ) {
        let group = self
            .group
            .as_ref()
            .expect("a frame uploads before it draws");
        rec.vertex(0, &self.instances.buffer);
        for run in Runs::new(items, |i| ids[i as usize]) {
            rec.pipeline(self.pipelines[run.key.idx()].select(stencil));
            rec.group(0, group);
            rec.draw(0..4, run.instances.u32_range());
        }
    }
}
