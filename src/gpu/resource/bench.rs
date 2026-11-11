//! What a gradient upload pays for one more copy against one more staged row, which sets
//! `MERGE_GAP_ROWS`.

use crate::bench::backends::Backends;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::bench_gpu::Timing;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::resource::gpu_gradient_atlas;
use crate::gpu::resource::upload_staging::UploadStaging;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::cpu_gradient_atlas::bake::LUT_ROW_TEXELS;
use criterion::{Criterion, Throughput};
use std::ops::Range;

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
pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
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
                    ctx.copy_rows(gpu_gradient_atlas::row_copies(texture, &runs, |run| {
                        &source[(run.start * ROW_BYTES) as usize..(run.end * ROW_BYTES) as usize]
                    }));
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
