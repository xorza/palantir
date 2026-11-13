//! [`MoveDraw`]: a list's moves drawn onto their destination page, one instanced draw a run of a
//! source and a destination, as WebRender's `ps_copy` moves texture-cache items.

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::texture_format;
use crate::gpu::atlas::page_list::Move;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::resource::texture_binding::{self, Filtering};
use std::mem::offset_of;

/// One move as the draw reads it: its origin on the source page, its origin on the destination
/// page, its size, in texels, and the destination page's edge, which places it in clip space.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MoveRect {
    pub(crate) src: [u32; 2],
    pub(crate) dst: [u32; 2],
    pub(crate) size: [u32; 2],
    pub(crate) dest_edge: u32,
}

impl MoveRect {
    /// `m` onto a destination page of `dest_edge`.
    pub(crate) fn of(m: Move, dest_edge: u32) -> Self {
        debug_assert_eq!(m.from.size, m.to.size, "a move keeps its item's size");
        Self {
            src: m.from.origin.as_uvec2().to_array(),
            dst: m.to.origin.as_uvec2().to_array(),
            size: m.from.size.as_uvec2().to_array(),
            dest_edge,
        }
    }
}

const ATTRS: [wgpu::VertexAttribute; 4] = [
    vertex_attr(0, wgpu::VertexFormat::Uint32x2, offset_of!(MoveRect, src)),
    vertex_attr(1, wgpu::VertexFormat::Uint32x2, offset_of!(MoveRect, dst)),
    vertex_attr(2, wgpu::VertexFormat::Uint32x2, offset_of!(MoveRect, size)),
    vertex_attr(
        3,
        wgpu::VertexFormat::Uint32,
        offset_of!(MoveRect, dest_edge),
    ),
];

/// A quad over each destination rectangle, whose fragments load the source texel at the same
/// offset: a load and a store of the texel as stored, so a draw moves the bytes a copy would, as
/// `atomic/atlas_move` checks on each device it runs.
const SHADER: ShaderBody = ShaderBody {
    label: "palantir.atlas.move",
    wgsl: "
@group(0) @binding(0) var source: texture_2d<f32>;

struct MoveIn {
    @builtin(vertex_index) idx: u32,
    @location(0) src: vec2<u32>,
    @location(1) dst: vec2<u32>,
    @location(2) size: vec2<u32>,
    @location(3) dest_edge: u32,
}

struct MoveOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) offset: vec2<i32>,
}

@vertex
fn vs(in: MoveIn) -> MoveOut {
    let corner = vec2<u32>(in.idx & 1u, (in.idx >> 1u) & 1u);
    let at = vec2<f32>(in.dst + in.size * corner) / f32(in.dest_edge);
    var out: MoveOut;
    out.position = vec4<f32>(at.x * 2.0 - 1.0, 1.0 - at.y * 2.0, 0.0, 1.0);
    out.offset = vec2<i32>(in.src) - vec2<i32>(in.dst);
    return out;
}

@fragment
fn fs(in: MoveOut) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(in.position.xy) + in.offset, 0);
}
",
    includes: &[],
    constants: &[],
};

/// The draw that moves a list's items: its shader, the source's layout, a pipeline for each list's
/// format, built on the first move of that list, and the buffer every run of a submission writes
/// its rectangles into, each at its own place.
#[derive(Debug)]
pub(crate) struct MoveDraw {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// By [`AtlasFormat`].
    pipelines: [Option<wgpu::RenderPipeline>; 3],
    /// Grown to the most rectangles one submission moved, and kept: its bound is the moves of the
    /// largest commit, a purge's, which a list's items bound.
    rects: Option<wgpu::Buffer>,
    /// The bytes of `rects` the runs since [`Self::begin`] took.
    used: u64,
}

/// One run of moves a draw records: from `source` onto `dest`, a page of `format`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveRun<'a> {
    pub(crate) format: AtlasFormat,
    pub(crate) source: &'a wgpu::TextureView,
    pub(crate) dest: &'a wgpu::TextureView,
    pub(crate) rects: &'a [MoveRect],
}

impl MoveDraw {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.atlas.move.bgl"),
                entries: &[texture_binding::texture_entry(0, Filtering::Loaded)],
            });
        Self {
            shader: SHADER.module(gpu),
            pipeline_layout: pipeline_recipe::pipeline_layout(
                gpu,
                "palantir.atlas.move.pl",
                &[Some(&layout)],
            ),
            layout,
            pipelines: [const { None }; 3],
            rects: None,
            used: 0,
        }
    }

    /// Start a submission's runs: each [`Self::record`] after takes the next place in the
    /// rectangles' buffer, as the queue writes every run's before the submission draws any.
    pub(crate) const fn begin(&mut self) {
        self.used = 0;
    }

    /// Record `run` on `encoder`: one pass over its destination, which keeps the page's other
    /// texels, and one instanced draw. The source's group is made for the run, as moves come only
    /// on the frames a drain runs.
    pub(crate) fn record(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        run: MoveRun<'_>,
    ) {
        let MoveRun {
            format,
            source,
            dest,
            rects,
        } = run;
        debug_assert!(!rects.is_empty(), "a run moves an item");
        let pipeline = self.pipelines[format as usize].get_or_insert_with(|| {
            PipelineRecipe {
                vertex_buffers: &[Some(pipeline_recipe::vertex_layout(
                    size_of::<MoveRect>(),
                    wgpu::VertexStepMode::Instance,
                    &ATTRS,
                ))],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                blend: None,
                ..PipelineRecipe::color(
                    "palantir.atlas.move",
                    &self.shader,
                    &self.pipeline_layout,
                    texture_format(format),
                )
            }
            .build(gpu)
            .pipeline
        });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.atlas.move.source"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        });
        let bytes = size_of_val(rects) as u64;
        let fits = self
            .rects
            .as_ref()
            .is_some_and(|buffer| self.used + bytes <= buffer.size());
        if !fits {
            // The runs recorded before keep the buffer they read until the submission is done.
            let size = (self.used + bytes).next_power_of_two();
            self.rects = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.atlas.move.rects"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.used = 0;
        }
        let instances = self.rects.as_ref().expect("made above");
        let at = self.used;
        gpu.queue
            .write_buffer(instances, at, bytemuck::cast_slice(rects));
        self.used += bytes;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.atlas.move.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dest,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, instances.slice(at..at + bytes));
        pass.draw(
            0..4,
            0..u32::try_from(rects.len()).expect("a run's moves fit u32"),
        );
    }
}
