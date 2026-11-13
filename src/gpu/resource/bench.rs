//! What texture memory costs to fill and to rearrange:
//!
//! - `atomic/gradient_upload`: one more copy against one more staged row, which sets
//!   `MERGE_GAP_ROWS`.
//! - `atomic/atlas_move`: rectangles moved from one atlas page to another, a copy command each
//!   against the pool's one instanced draw ([`MoveDraw`]), the two routes a device moves items
//!   by (`src/gpu/AGENTS.md`, Atlases).
//! - `atomic/atlas_page`: a page made and first written, what a pool pays to open one.

use crate::bench::backends::{Backend, Backends};
use crate::bench::kind::{Kind, Scale};
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::move_draw::{MoveDraw, MoveRect, MoveRun};
use crate::gpu::atlas::atlas_textures::texture_format;
use crate::gpu::atlas::row_texture;
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::upload_staging::UploadStaging;
use crate::gpu::surface::texel_readback::TexelReadback;
use crate::gpu::texel;
use crate::gpu::texel::TargetSpec;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::row_table::bake::LUT_ROW_TEXELS;
use criterion::{Criterion, Throughput};
use glam::UVec2;
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
                    ctx.copy_rows(row_texture::row_copies(texture, copies));
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

/// The atlas pool's page formats, each with its part of a case's id: the
/// masks, the color pages as `Rgba8Unorm`, and the backdrop results.
const PAGE_FORMATS: [(AtlasFormat, &str); 3] = [
    (AtlasFormat::R8, "r8"),
    (AtlasFormat::Rgba8, "rgba8"),
    (AtlasFormat::Rgba16F, "rgba16f"),
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

/// Two pages of one format, the source filled with a pattern, and the pool's draw that moves
/// between them.
#[derive(Debug)]
struct PagePair {
    format: AtlasFormat,
    source: wgpu::Texture,
    source_view: wgpu::TextureView,
    dest: wgpu::Texture,
    dest_view: wgpu::TextureView,
    draw: MoveDraw,
    /// Zeros for a whole page, which clear the destination before a case checks its move.
    zeros: Vec<u8>,
}

impl PagePair {
    fn new(gpu: &Gpu, format: AtlasFormat) -> Self {
        let page = |label| {
            texel::texture_2d(
                gpu,
                TargetSpec {
                    label,
                    size: UVec2::splat(MOVE_PAGE),
                    format: texture_format(format),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST,
                },
            )
        };
        let source = page("palantir.bench.atlas_move.source");
        let dest = page("palantir.bench.atlas_move.dest");
        let bytes = (MOVE_PAGE * MOVE_PAGE * format.texel_bytes()) as usize;
        // Every byte apart from its neighbors, and no f16 a NaN, whose bits a draw may not keep:
        // a byte below 0x7c in each half's high byte keeps the exponent off its all-ones. Some
        // halves are subnormal, so a GPU that flushes them in a draw fails the check, which then
        // says the draw does not move results exactly there.
        let pattern: Vec<u8> = (0..bytes).map(|i| (i % 0x7b) as u8).collect();
        write_page(gpu, &source, &pattern);
        let view =
            |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            format,
            source_view: view(&source),
            dest_view: view(&dest),
            source,
            dest,
            draw: MoveDraw::new(gpu),
            zeros: vec![0; bytes],
        }
    }

    /// Record `moves` by `mover` on `encoder`.
    fn record(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        mover: Mover,
        moves: &[MoveRect],
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
                self.draw.begin();
                self.draw.record(
                    gpu,
                    encoder,
                    MoveRun {
                        format: self.format,
                        source: &self.source_view,
                        dest: &self.dest_view,
                        rects: moves,
                    },
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
    fn check(&mut self, bench: &BenchGpu, mover: Mover, moves: &[MoveRect]) {
        let gpu = &bench.gpu;
        write_page(gpu, &self.dest, &self.zeros);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.record(gpu, &mut encoder, mover, moves);
        gpu.queue.submit([encoder.finish()]);
        bench.wait();
        let source = read_page(gpu, &self.source);
        let dest = read_page(gpu, &self.dest);
        let texel = self.format.texel_bytes() as usize;
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
fn moves(side: u32, count: u32) -> Vec<MoveRect> {
    let cols = MOVE_PAGE / side;
    assert!(
        count <= cols * cols,
        "{count} rectangles of {side} fill no page"
    );
    let cell = |i: u32| [i % cols * side, i / cols * side];
    (0..count)
        .map(|i| MoveRect {
            src: cell(i),
            dst: cell(count - 1 - i),
            size: [side, side],
            dest_edge: MOVE_PAGE,
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
                        let pair = pages.as_mut().expect("made above");
                        let moves = moves(side, count);
                        if !checked {
                            pair.check(bench, mover, &moves);
                            checked = true;
                        }
                        b.iter(|| {
                            let mut encoder = gpu
                                .device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                            pair.record(gpu, &mut encoder, mover, &moves);
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
                    vec![0x55u8; (FIRST_WRITE * FIRST_WRITE * format.texel_bytes()) as usize];
                b.iter(|| {
                    let page = texel::texture_2d(
                        gpu,
                        TargetSpec {
                            label: "palantir.bench.atlas_page",
                            size: UVec2::splat(edge),
                            format: texture_format(format),
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
                            bytes_per_row: Some(FIRST_WRITE * format.texel_bytes()),
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
