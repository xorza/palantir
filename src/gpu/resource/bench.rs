//! What texture memory costs to fill and to rearrange:
//!
//! - `atomic/gradient_upload`: one more copy against one more staged row, which sets
//!   `MERGE_GAP_ROWS`.
//! - `atomic/atlas_move`: rectangles moved from one atlas page to another, a copy command each
//!   against one instanced draw, which picks how `.notes/proposals/atlas-pool.md` moves items.
//! - `atomic/atlas_page`: a page made and first written, what a pool pays to open one.

use crate::bench::backends::{Backend, Backends};
use crate::bench::kind::{Kind, Scale};
use crate::gpu::atlas::row_table;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::device::gpu::{FilledBuffer, Gpu};
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe, vertex_attr};
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::resource::texture_binding::{self, Filtering};
use crate::gpu::resource::upload_staging::UploadStaging;
use crate::gpu::surface::owned_target::TargetSpec;
use crate::gpu::surface::texel_readback::TexelReadback;
use crate::gpu::texel;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::cpu_gradient_atlas::bake::LUT_ROW_TEXELS;
use criterion::{Criterion, Throughput};
use glam::UVec2;
use std::mem::offset_of;
use std::ops::Range;

/// The cases of this file.
pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    gradient_upload(c, backends, backend);
    atlas_move(c, backends, backend);
    atlas_page(c, backends, backend);
}

/// Dirty rows each case uploads.
const DIRTY: u32 = 32;

/// Clean rows between two dirty ones.
const GAPS: [u32; 5] = [16, 32, 48, 64, 128];

const ROW_BYTES: u32 = (LUT_ROW_TEXELS * size_of::<RgbaF16>()) as u32;

/// How the dirty rows reach the texture.
#[derive(Clone, Copy, Debug)]
enum Upload {
    /// One copy per dirty row.
    Split,
    /// One copy over every dirty row and the gaps between them.
    Merged,
}

/// What a case uploads into: the atlas texture, tall enough for the widest gap, the staging belt,
/// and the rows' source bytes.
#[derive(Debug)]
struct Fixture {
    texture: wgpu::Texture,
    belt: UploadStaging,
    source: Vec<u8>,
}

impl Fixture {
    fn new(gpu: &Gpu) -> Self {
        let rows = DIRTY + (DIRTY - 1) * GAPS[GAPS.len() - 1];
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.bench.gradient_upload"),
            size: wgpu::Extent3d {
                width: LUT_ROW_TEXELS as u32,
                height: rows,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self {
            texture,
            belt: UploadStaging::new(&gpu.device, 1 << 20),
            source: vec![0u8; (rows * ROW_BYTES) as usize],
        }
    }
}

/// `atomic/gradient_upload/gpu/<split|merged>/gap<g>`: [`DIRTY`] rows `g` clean rows apart,
/// staged in the belt and copied into the atlas texture as `split` or `merged` says, then
/// submitted and waited for. Per dirty row. Where `merged` stops being cheaper is the gap a
/// flush should fold into a run.
fn gradient_upload(c: &mut Criterion, backends: &Backends, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "gradient_upload", Scale::Sample);
    group.throughput(Throughput::Elements(u64::from(DIRTY)));
    for gap in GAPS {
        for upload in [Upload::Split, Upload::Merged] {
            let name = match upload {
                Upload::Split => "split",
                Upload::Merged => "merged",
            };
            let runs: Vec<Range<u32>> = match upload {
                Upload::Split => (0..DIRTY)
                    .map(|i| i * (gap + 1)..i * (gap + 1) + 1)
                    .collect(),
                Upload::Merged => {
                    let span = 0..DIRTY + (DIRTY - 1) * gap;
                    vec![span]
                }
            };
            // Criterion calls the closure once per warm-up round and per sample: the fixture is
            // built on the first, so every timed iteration finds the belt's chunks allocated.
            let mut fixture = None;
            group.bench_function(backend.case(&format!("{name}/gap{gap}")), |b| {
                let bench = backends.device(Timing::Bare);
                let gpu = bench.handles();
                let Fixture {
                    texture,
                    belt,
                    source,
                } = fixture.get_or_insert_with(|| Fixture::new(&gpu));
                b.iter(|| {
                    let mut encoder = gpu
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    // Frame 0 throughout: a staging case ends no expiry window.
                    let mut ctx = GpuCtx::new(&gpu, belt, &mut encoder, 0);
                    let copies = runs.iter().map(|run| {
                        let bytes = &source
                            [(run.start * ROW_BYTES) as usize..(run.end * ROW_BYTES) as usize];
                        (run.clone(), bytes)
                    });
                    ctx.copy_rows(row_table::row_copies(texture, copies));
                    belt.finish();
                    gpu.queue.submit([encoder.finish()]);
                    belt.recall();
                    bench.wait();
                });
            });
        }
    }
    group.finish();
}

/// The page formats of `.notes/proposals/atlas-pool.md`, each with its part of a case's id: the
/// masks, the color pages as `Rgba8Unorm`, and the backdrop results.
const PAGE_FORMATS: [(wgpu::TextureFormat, &str); 3] = [
    (wgpu::TextureFormat::R8Unorm, "r8"),
    (wgpu::TextureFormat::Rgba8Unorm, "rgba8"),
    (wgpu::TextureFormat::Rgba16Float, "rgba16f"),
];

/// The edge of the pages a move runs between, the largest page the proposal opens.
const MOVE_PAGE: u32 = 2048;

/// The sides of the rectangles a move takes: a glyph's, and an icon's or a small backdrop's.
const MOVE_SIDES: [u32; 2] = [24, 128];

/// How many rectangles one frame moves: up to the 512² texels a frame WebRender's compaction
/// moves, which 16 rectangles of 128² reach, and past it for the glyphs.
const MOVE_COUNTS: [u32; 3] = [16, 64, 256];

/// What moves the rectangles.
#[derive(Clone, Copy, Debug)]
enum Mover {
    /// A `copy_texture_to_texture` a rectangle.
    Copy,
    /// One instanced draw over the destination page, each fragment loading its source texel.
    Draw,
}

impl Mover {
    const fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Draw => "draw",
        }
    }
}

/// One rectangle a move takes, as [`MOVE_SHADER`] reads it: its origin on the source page, its
/// origin on the destination page and its size, in texels.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
struct Move {
    src: [u32; 2],
    dst: [u32; 2],
    size: [u32; 2],
}

const MOVE_ATTRS: [wgpu::VertexAttribute; 3] = [
    vertex_attr(0, wgpu::VertexFormat::Uint32x2, offset_of!(Move, src)),
    vertex_attr(1, wgpu::VertexFormat::Uint32x2, offset_of!(Move, dst)),
    vertex_attr(2, wgpu::VertexFormat::Uint32x2, offset_of!(Move, size)),
];

/// The draw that moves rectangles, as WebRender's `ps_copy` copies texture-cache items: a quad over
/// each destination rectangle, whose fragments load the source texel at the same offset.
const MOVE_SHADER: ShaderBody = ShaderBody {
    label: "palantir.bench.atlas_move",
    wgsl: "
const PAGE: f32 = /*{PAGE}*/;

@group(0) @binding(0) var source: texture_2d<f32>;

struct MoveIn {
    @builtin(vertex_index) idx: u32,
    @location(0) src: vec2<u32>,
    @location(1) dst: vec2<u32>,
    @location(2) size: vec2<u32>,
}

struct MoveOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) offset: vec2<i32>,
}

@vertex
fn vs(in: MoveIn) -> MoveOut {
    let corner = vec2<u32>(in.idx & 1u, (in.idx >> 1u) & 1u);
    let at = vec2<f32>(in.dst + in.size * corner) / PAGE;
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
    constants: &[ShaderConstant::float("PAGE", MOVE_PAGE as f32)],
};

/// Two pages of one format, the source filled with a pattern, and the draw that moves between them.
#[derive(Debug)]
struct PagePair {
    format: wgpu::TextureFormat,
    source: wgpu::Texture,
    dest: wgpu::Texture,
    dest_view: wgpu::TextureView,
    group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    /// Zeros for a whole page, which clear the destination before a case checks its move.
    zeros: Vec<u8>,
}

impl PagePair {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let page = |label| {
            texel::texture_2d(
                gpu,
                TargetSpec {
                    label,
                    size: UVec2::splat(MOVE_PAGE),
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST,
                },
            )
        };
        let source = page("palantir.bench.atlas_move.source");
        let dest = page("palantir.bench.atlas_move.dest");
        let bytes = (MOVE_PAGE * MOVE_PAGE * texel_bytes(format)) as usize;
        // Every byte apart from its neighbors, and no f16 a NaN, whose bits a draw may not keep:
        // a byte below 0x7c in each half's high byte keeps the exponent off its all-ones. Some
        // halves are subnormal, so a GPU that flushes them in a draw fails the check, which then
        // says the draw does not move results exactly there.
        let pattern: Vec<u8> = (0..bytes).map(|i| (i % 0x7b) as u8).collect();
        write_page(gpu, &source, &pattern);
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.bench.atlas_move.bgl"),
                entries: &[texture_binding::texture_entry(0, Filtering::Loaded)],
            });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.bench.atlas_move.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &source.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            }],
        });
        let shader = MOVE_SHADER.module(gpu);
        let pipeline_layout =
            pipeline_recipe::pipeline_layout(gpu, "palantir.bench.atlas_move.pl", &[Some(&layout)]);
        let pipeline = PipelineRecipe {
            vertex_buffers: &[Some(pipeline_recipe::vertex_layout(
                size_of::<Move>(),
                wgpu::VertexStepMode::Instance,
                &MOVE_ATTRS,
            ))],
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            blend: None,
            ..PipelineRecipe::color(
                "palantir.bench.atlas_move",
                &shader,
                &pipeline_layout,
                format,
            )
        }
        .build(gpu)
        .pipeline;
        Self {
            format,
            dest_view: dest.create_view(&wgpu::TextureViewDescriptor::default()),
            source,
            dest,
            group,
            pipeline,
            zeros: vec![0; bytes],
        }
    }

    /// Record `moves` by `mover` on `encoder`.
    fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        mover: Mover,
        moves: &[Move],
        instances: &wgpu::Buffer,
    ) {
        match mover {
            Mover::Copy => {
                for m in moves {
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            origin: texel::origin(UVec2::from(m.src)),
                            ..self.source.as_image_copy()
                        },
                        wgpu::TexelCopyTextureInfo {
                            origin: texel::origin(UVec2::from(m.dst)),
                            ..self.dest.as_image_copy()
                        },
                        texel::extent(UVec2::from(m.size)),
                    );
                }
            }
            Mover::Draw => {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("palantir.bench.atlas_move.pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.dest_view,
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
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.group, &[]);
                pass.set_vertex_buffer(0, instances.slice(..));
                pass.draw(
                    0..4,
                    0..u32::try_from(moves.len()).expect("a move count fits u32"),
                );
            }
        }
    }

    /// Check that `moves`, run once by `mover` on a cleared destination, land every source texel
    /// at its new place.
    ///
    /// # Panics
    ///
    /// Panics where a destination texel differs from its source.
    fn check(&self, bench: &BenchGpu, mover: Mover, moves: &[Move], instances: &wgpu::Buffer) {
        let gpu = &bench.gpu;
        write_page(gpu, &self.dest, &self.zeros);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.record(&mut encoder, mover, moves, instances);
        gpu.queue.submit([encoder.finish()]);
        bench.wait();
        let source = read_page(gpu, &self.source);
        let dest = read_page(gpu, &self.dest);
        let texel = texel_bytes(self.format) as usize;
        let row = MOVE_PAGE as usize * texel;
        for m in moves {
            let width = m.size[0] as usize * texel;
            for y in 0..m.size[1] as usize {
                let at =
                    |origin: [u32; 2]| (origin[1] as usize + y) * row + origin[0] as usize * texel;
                assert_eq!(
                    dest[at(m.dst)..at(m.dst) + width],
                    source[at(m.src)..at(m.src) + width],
                    "{:?} by {}: row {y} of the move from {:?}",
                    self.format,
                    mover.label(),
                    m.src
                );
            }
        }
    }
}

/// The bytes of one texel of `format`.
fn texel_bytes(format: wgpu::TextureFormat) -> u32 {
    format
        .block_copy_size(None)
        .expect("a page holds a color format")
}

/// Write `bytes`, a whole page, into `page`.
fn write_page(gpu: &Gpu, page: &wgpu::Texture, bytes: &[u8]) {
    gpu.queue.write_texture(
        page.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(page.width() * texel_bytes(page.format())),
            rows_per_image: None,
        },
        page.size(),
    );
}

/// Every byte of `page`, waited for.
fn read_page(gpu: &Gpu, page: &wgpu::Texture) -> Vec<u8> {
    let mut texels = Vec::new();
    let read = TexelReadback::new(gpu, page).read(gpu, page, &mut texels);
    assert_eq!(read, GpuWait::Done, "the page reads back");
    texels
}

/// `count` rectangles of `side`: the `i`th from the `i`th cell of a grid on the source page to the
/// `count - 1 - i`th on the destination, so each lands elsewhere.
fn moves(side: u32, count: u32) -> Vec<Move> {
    let cols = MOVE_PAGE / side;
    assert!(
        count <= cols * cols,
        "{count} rectangles of {side} fill no page"
    );
    let cell = |i: u32| [i % cols * side, i / cols * side];
    (0..count)
        .map(|i| Move {
            src: cell(i),
            dst: cell(count - 1 - i),
            size: [side, side],
        })
        .collect()
}

/// `atomic/atlas_move/gpu/<copy|draw>/<format>/<side>/<count>`: `count` rectangles of `side`²
/// texels moved between two pages of `format`, recorded, submitted and waited for; wall time.
/// Per rectangle. Each case first checks its move lands every texel.
fn atlas_move(c: &mut Criterion, backends: &Backends, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "atlas_move", Scale::Sample);
    // One pair at a time, the cases of a format in a row: three pairs of 2048² RGBA16F alive at
    // once would hold 192 MiB.
    let mut pages: Option<PagePair> = None;
    for (format, format_id) in PAGE_FORMATS {
        for side in MOVE_SIDES {
            for count in MOVE_COUNTS {
                group.throughput(Throughput::Elements(u64::from(count)));
                for mover in [Mover::Copy, Mover::Draw] {
                    let id = format!("{}/{format_id}/{side}/{count}", mover.label());
                    let mut checked = false;
                    group.bench_function(backend.case(&id), |b| {
                        let bench = backends.device(Timing::Bare);
                        let gpu = &bench.gpu;
                        if pages.as_ref().is_none_or(|pair| pair.format != format) {
                            pages = None;
                            pages = Some(PagePair::new(gpu, format));
                        }
                        let pair = pages.as_ref().expect("made above");
                        let moves = moves(side, count);
                        let instances = gpu.create_filled_buffer(
                            FilledBuffer {
                                label: "palantir.bench.atlas_move.instances",
                                size: size_of_val(moves.as_slice()) as u64,
                                usage: wgpu::BufferUsages::VERTEX,
                            },
                            &[bytemuck::cast_slice(&moves)],
                        );
                        if !checked {
                            pair.check(bench, mover, &moves, &instances);
                            checked = true;
                        }
                        b.iter(|| {
                            let mut encoder = gpu
                                .device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                            pair.record(&mut encoder, mover, &moves, &instances);
                            gpu.queue.submit([encoder.finish()]);
                            bench.wait();
                        });
                    });
                }
            }
        }
    }
    group.finish();
}

/// The edges a pool opens a page at: a list's first page, the color and half-float page edge, and
/// the mask page edge.
const PAGE_EDGES: [u32; 3] = [256, 1024, 2048];

/// The side of the raster each made page takes first.
const FIRST_WRITE: u32 = 64;

/// `atomic/atlas_page/gpu/<format>/<edge>`: a page of `format` and `edge`² made, a
/// [`FIRST_WRITE`]² raster written into it, submitted and waited for, so the time holds the
/// clear wgpu runs over a texture before its first partial write; wall time.
fn atlas_page(c: &mut Criterion, backends: &Backends, backend: Backend) {
    let mut group = Kind::Atomic.group(c, "atlas_page", Scale::Sample);
    for (format, format_id) in PAGE_FORMATS {
        for edge in PAGE_EDGES {
            group.bench_function(backend.case(&format!("{format_id}/{edge}")), |b| {
                let bench = backends.device(Timing::Bare);
                let gpu = &bench.gpu;
                let raster =
                    vec![0x55u8; (FIRST_WRITE * FIRST_WRITE * texel_bytes(format)) as usize];
                b.iter(|| {
                    let page = texel::texture_2d(
                        gpu,
                        TargetSpec {
                            label: "palantir.bench.atlas_page",
                            size: UVec2::splat(edge),
                            format,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::COPY_SRC
                                | wgpu::TextureUsages::COPY_DST,
                        },
                    );
                    gpu.queue.write_texture(
                        page.as_image_copy(),
                        &raster,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(FIRST_WRITE * texel_bytes(format)),
                            rows_per_image: None,
                        },
                        texel::extent(UVec2::splat(FIRST_WRITE)),
                    );
                    gpu.queue.submit([]);
                    bench.wait();
                    assert_eq!(page.width(), edge, "the page has its edge");
                    page
                });
            });
        }
    }
    group.finish();
}
