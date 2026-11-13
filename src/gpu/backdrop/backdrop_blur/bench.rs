//! The backdrop blur's device benches, in device time between passes.
//!
//! - `atomic/backdrop_blur/gpu/<σ>/<side>`: one blur of a whole `side`² surface at σ physical px,
//!   from its capture into its result, a scratch texture, as a result past half a page takes.
//!   Each σ names its levels: 1 and 4 px stay at full resolution, 16 px halves twice, 48 px four
//!   times and 160 px six.
//! - `atomic/backdrop_blur/gpu/jobs/<n>`: `n` blurs of 8 × 8 texels at σ 32 px, three halvings,
//!   apart on a 2048² surface, in one set of passes, as backdrops that share a break blur, into a
//!   page of the half-float list's edge, where small results lie: the cost a set of passes pays
//!   whatever it blurs, the destination's whole-page pass or its staging and copies among it.

use crate::bench::backends::Backends;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::backdrop::backdrop_blur::{
    BackdropBlur, BlurJob, JobLayout, LEVEL_FORMAT, ResultTarget,
};
use crate::gpu::bench_gpu::{BenchGpu, PassTimer, TARGET_FORMAT, Timing};
use crate::gpu::pipeline::effect_pipeline::backdrop_groups::ResultTexture;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::upload_staging::UploadStaging;
use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use criterion::{Bencher, Criterion, Throughput};
use glam::UVec2;
use std::time::Duration;

/// Each σ and the halvings it takes.
const SIGMAS: [(u32, u8); 5] = [(1, 0), (4, 0), (16, 2), (48, 4), (160, 6)];
const SIDES: [u32; 2] = [512, 2048];

/// A capture of noise over a `side`² surface, so no pass reads a constant.
fn fill_capture(gpu: &BenchGpu, blur: &mut BackdropBlur, side: u32) {
    let surface = UVec2::splat(side);
    blur.capture_view(&gpu.gpu, TARGET_FORMAT, surface);
    let texture = &blur.captures[0].target.texture;
    let pixels: Vec<u8> = (0..side * side * 4)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    gpu.gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 4),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
    );
}

pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "backdrop_blur", Scale::Frame);
    for (sigma, levels) in SIGMAS {
        for side in SIDES {
            group.throughput(Throughput::Elements(u64::from(side * side)));
            group.bench_function(backend.case(&format!("{sigma}/{side}")), |bencher| {
                let gpu = backends.device(Timing::PassOnly);
                let plan = BlurPlan::new(sigma as f32);
                assert_eq!(plan.levels(), levels, "σ {sigma} px halves {levels} times");
                let surface = UVec2::splat(side);
                let region = plan
                    .region(URect::new(0, 0, side, side), surface)
                    .expect("the whole surface");
                let job = BlurJob {
                    plan,
                    surface,
                    format: TARGET_FORMAT,
                    footprint: URect::new(0, 0, side, side),
                    region,
                    dest: ResultTexture::Scratch(ScratchId {
                        format: AtlasFormat::Rgba16F,
                        idx: 0,
                        generation: 0,
                    }),
                    dest_origin: UVec2::ZERO,
                    layout: JobLayout::default(),
                };
                let mut blur = BackdropBlur::new(&gpu.gpu, TextureBinding::new(&gpu.gpu).sampler());
                fill_capture(gpu, &mut blur, side);
                let dest_size = job.dest_rect().max();
                let dest = gpu.gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("palantir.bench.backdrop_blur.dest"),
                    size: wgpu::Extent3d {
                        width: dest_size.x,
                        height: dest_size.y,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: LEVEL_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = dest.create_view(&wgpu::TextureViewDescriptor::default());
                time_jobs(
                    bencher,
                    gpu,
                    &mut blur,
                    &mut [job],
                    ResultTarget {
                        texture: &dest,
                        view: &view,
                    },
                );
            });
        }
    }
    for n in [1u32, 8] {
        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_function(backend.case(&format!("jobs/{n}")), |bencher| {
            let gpu = backends.device(Timing::PassOnly);
            let plan = BlurPlan::new(32.0);
            assert_eq!(plan.levels(), 3, "σ 32 px halves three times");
            let mut jobs: Vec<BlurJob> = (0..n)
                .map(|i| BlurJob {
                    plan,
                    surface: UVec2::splat(2048),
                    format: TARGET_FORMAT,
                    footprint: URect::new(0, 0, 2048, 2048),
                    region: URect::new(16 * i, 8, 8, 8),
                    dest: ResultTexture::Page(0),
                    dest_origin: UVec2::new(16 * i, 0),
                    layout: JobLayout::default(),
                })
                .collect();
            let mut blur = BackdropBlur::new(&gpu.gpu, TextureBinding::new(&gpu.gpu).sampler());
            fill_capture(gpu, &mut blur, 2048);
            let dest_size = UVec2::splat(PageListConfig::of(AtlasFormat::Rgba16F).page_edge);
            assert!(
                jobs.iter()
                    .all(|job| job.dest_rect().max().cmple(dest_size).all()),
                "every result lies on the page"
            );
            let dest = gpu.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("palantir.bench.backdrop_blur.jobs"),
                size: wgpu::Extent3d {
                    width: dest_size.x,
                    height: dest_size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: LEVEL_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = dest.create_view(&wgpu::TextureViewDescriptor::default());
            time_jobs(
                bencher,
                gpu,
                &mut blur,
                &mut jobs,
                ResultTarget {
                    texture: &dest,
                    view: &view,
                },
            );
        });
    }
    group.finish();
}

/// Time `blur` running `jobs` into `dest`, a frame each.
fn time_jobs(
    bencher: &mut Bencher<'_>,
    gpu: &BenchGpu,
    blur: &mut BackdropBlur,
    jobs: &mut [BlurJob],
    dest: ResultTarget<'_>,
) {
    let mut timer = PassTimer::new(gpu);
    let mut belt = UploadStaging::new(&gpu.gpu.device, 1 << 16);
    let handles = gpu.handles();
    let mut frame = 0;
    let mut once = || {
        frame += 1;
        let elapsed = timer.time(gpu, |encoder| {
            let mut ctx = GpuCtx::new(&handles, &mut belt, encoder, frame);
            blur.run(&mut ctx, jobs, dest);
            belt.finish();
        });
        belt.recall();
        elapsed
    };
    once();
    bencher.iter_custom(|iters| (0..iters).map(|_| once()).sum::<Duration>());
}
