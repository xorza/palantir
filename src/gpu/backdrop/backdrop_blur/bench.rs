//! `atomic/backdrop_blur/gpu/<σ>/<side>`: one blur of a whole `side`² surface at σ physical px,
//! from its capture into its result, in device time between passes. Each σ names its levels: 1
//! and 4 px stay at full resolution, 16 px halves twice, 48 px four times and 160 px six.

use crate::bench::backends::Backends;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, LEVEL_FORMAT};
use crate::gpu::bench_gpu::{BenchGpu, PassTimer, TARGET_FORMAT, Timing};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use criterion::{Criterion, Throughput};
use glam::UVec2;
use std::time::Duration;
use wgpu::util::StagingBelt;

/// Each σ and the halvings it takes.
const SIGMAS: [(u32, u8); 5] = [(1, 0), (4, 0), (16, 2), (48, 4), (160, 6)];
const SIDES: [u32; 2] = [512, 2048];

/// A capture of noise over a `side`² surface, so no pass reads a constant.
fn fill_capture(gpu: &BenchGpu, blur: &mut BackdropBlur, side: u32) {
    let surface = UVec2::splat(side);
    blur.capture_view(&gpu.gpu.device, TARGET_FORMAT, surface);
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

pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "backdrop_blur", Scale::Frame);
    for (sigma, levels) in SIGMAS {
        for side in SIDES {
            group.throughput(Throughput::Elements(u64::from(side * side)));
            group.bench_function(backend.case(&format!("{sigma}/{side}")), |bencher| {
                let gpu = BenchGpu::shared(Timing::PassOnly);
                let mut timer = PassTimer::new(gpu);
                let device = &gpu.gpu.device;
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
                    region,
                    dest_origin: UVec2::ZERO,
                };
                let mut blur = BackdropBlur::new(device);
                fill_capture(gpu, &mut blur, side);
                let dest_size = job.dest_rect().max();
                let dest = device.create_texture(&wgpu::TextureDescriptor {
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
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let view = dest.create_view(&wgpu::TextureViewDescriptor::default());
                let mut belt = StagingBelt::new(device.clone(), 1 << 16);
                let handles = gpu.handles();
                let mut once = || {
                    let elapsed = timer.time(gpu, |encoder| {
                        let mut ctx = GpuCtx::new(&handles, &mut belt, encoder);
                        blur.run(&mut ctx, &[job], &view);
                        belt.finish();
                    });
                    belt.recall();
                    elapsed
                };
                once();
                bencher.iter_custom(|iters| (0..iters).map(|_| once()).sum::<Duration>());
            });
        }
    }
    group.finish();
}
