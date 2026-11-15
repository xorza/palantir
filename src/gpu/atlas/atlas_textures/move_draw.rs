//! [`MoveDraw`]: a list's moves drawn onto their destination page, one instanced draw a run of a
//! source and a destination, as WebRender's `ps_copy` moves texture-cache items.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
use crate::gpu::atlas::atlas_textures::texture_format;
use crate::gpu::atlas::page_list::{MAX_PAGES, Move};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::resource::shader_body::ShaderBody;
use crate::gpu::resource::texture_binding::{self, Filtering};
use std::mem::offset_of;
use std::ops::Range;

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
/// format, built on the first move of that list, and a commit's rectangles, every run's one after
/// another in a buffer that decays as per-frame scratch does (`src/gpu/AGENTS.md`, GPU memory).
#[derive(Debug)]
pub(crate) struct MoveDraw {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// By [`AtlasFormat`].
    pipelines: PerFormat<Option<wgpu::RenderPipeline>>,
    /// The commit's rectangles, written once ahead of its runs.
    rects: DynamicBuffer<MoveRect>,
    /// The commit's rectangles as [`Self::stage`] gathers them, kept across commits.
    staged: Vec<MoveRect>,
    /// Where each run's rectangles lie in `rects`, by list and destination page; `None` where no
    /// move of the commit lands there.
    runs: PerFormat<[Option<Range<u32>>; MAX_PAGES]>,
}

/// One run of moves a draw records: from `source` onto `dest`, page `page` of `format`'s list.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveRun<'a> {
    pub(crate) format: AtlasFormat,
    pub(crate) page: usize,
    pub(crate) source: &'a wgpu::TextureView,
    pub(crate) dest: &'a wgpu::TextureView,
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
            pipeline_layout: gpu.pipeline_layout("palantir.atlas.move.pl", &[Some(&layout)]),
            layout,
            pipelines: PerFormat::default(),
            rects: DynamicBuffer::vertex(gpu, "palantir.atlas.move.rects", 16),
            staged: Vec::new(),
            runs: PerFormat::default(),
        }
    }

    /// Gather the rectangles of every move `pool`'s lists wait to commit, a run a destination
    /// page, in the moves' order, and write them through `ctx` ahead of the runs that read them; a
    /// commit with no move writes nothing, and the buffer steps down with the window.
    pub(crate) fn stage(&mut self, ctx: &mut GpuCtx<'_>, pool: &AtlasPool) {
        self.staged.clear();
        for format in AtlasFormat::ALL {
            let list = pool.list(format);
            for page in 0..MAX_PAGES {
                let start = self.staged.len();
                for &m in list
                    .moves()
                    .iter()
                    .filter(|m| usize::from(m.to.page) == page)
                {
                    let edge = list
                        .page(page)
                        .expect("a move lands on an open page")
                        .edge();
                    self.staged.push(MoveRect::of(m, edge));
                }
                let end = self.staged.len();
                self.runs[format][page] = (end > start).then(|| {
                    u32::try_from(start).expect("a commit's moves fit u32")
                        ..u32::try_from(end).expect("a commit's moves fit u32")
                });
            }
        }
        self.rects.upload_instances(ctx, &self.staged);
    }

    /// Record `run`, which [`Self::stage`] gathered, on `encoder`: one pass over its destination,
    /// which keeps the page's other texels, and one instanced draw. The source's group is made for
    /// the run, as moves come only on the frames a drain runs.
    pub(crate) fn record(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        run: MoveRun<'_>,
    ) {
        let MoveRun {
            format,
            page,
            source,
            dest,
        } = run;
        let rects = self.runs[format][page]
            .clone()
            .expect("a recorded run was staged");
        let pipeline = self.pipelines[format].get_or_insert_with(|| {
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
        // The run's place in the buffer is its slice, not a first instance, which OpenGL ES has
        // no draw call for.
        let bytes = |at: u32| u64::from(at) * size_of::<MoveRect>() as u64;
        pass.set_vertex_buffer(
            0,
            self.rects
                .buffer
                .slice(bytes(rects.start)..bytes(rects.end)),
        );
        pass.draw(0..4, 0..rects.end - rects.start);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
    use crate::gpu::atlas::atlas_textures::move_draw::{MoveDraw, MoveRect};
    use crate::gpu::resource::gpu_ctx::GpuCtx;

    impl MoveDraw {
        /// Stage `rects` through `ctx` as the next commit's one run, onto page 0 of `format`'s
        /// list, as a bench moves rectangles no pool planned.
        pub(crate) fn stage_run(
            &mut self,
            ctx: &mut GpuCtx<'_>,
            format: AtlasFormat,
            rects: &[MoveRect],
        ) {
            self.staged.clear();
            self.staged.extend_from_slice(rects);
            self.runs = PerFormat::default();
            self.runs[format][0] =
                Some(0..u32::try_from(rects.len()).expect("a run's moves fit u32"));
            self.rects.upload_instances(ctx, &self.staged);
        }
    }
}
