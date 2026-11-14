//! The cutout tables' device bench, in device time between passes.
//!
//! - `atomic/cutout_tables/gpu/bake/<n>`: `n` tables of 87 texels a side, `(8, 16)`'s and its
//!   neighbors', baked onto one mask page at the list's page edge, as a frame that meets new
//!   shadows bakes: the bake's draws, and the page's whole-page pass or the staging and copies of
//!   a device whose passes move their whole target.

use crate::bench::backends::Backends;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::{AtlasPool, PoolConfig};
use crate::gpu::bench_gpu::{PassTimer, Timing};
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{BakeTable, CutoutKey};
use crate::gpu::pipeline::quad_pipeline::cutout_tables::CutoutTables;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::upload_staging::UploadStaging;
use criterion::{Criterion, Throughput};
use glam::U16Vec2;
use std::time::Duration;

/// A table's side at `(8, 16)`, `floor((8 + 2 · 64.5) · 10 / 16) + 2`, which its neighbors share.
const SIDE: u32 = 87;

pub(crate) fn atomic(c: &mut Criterion, backends: &Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "cutout_tables", Scale::Frame);
    for n in [1u32, 4] {
        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_function(backend.case(&format!("bake/{n}")), |bencher| {
            let gpu = backends.device(Timing::PassOnly);
            let edge = PageListConfig::of(AtlasFormat::R8).page_edge;
            let mut pool = AtlasPool::new(PoolConfig {
                lists: PerFormat::from_fn(|_| PageListConfig {
                    first_edge: edge,
                    page_edge: edge,
                    soft_bytes: 1 << 30,
                }),
                ..PoolConfig::of_device(gpu.gpu.max_texture_dimension().get(), PassTraffic::Drawn)
            });
            let shader = QuadPipeline::SHADER.module(&gpu.gpu);
            let mut tables = CutoutTables::new(&gpu.gpu, &shader);
            let mut placements = CutoutPlacements::default();
            let mut placing = CutoutPlacing {
                placements: &mut placements,
                pool: &mut pool,
            };
            tables.bakes = (0..n)
                .map(|i| {
                    let (r, sigma) = (8.0 + i as f32 * 0.25, 16.0);
                    BakeTable {
                        table: placing
                            .acquire(CutoutKey::new(r, sigma), SIDE)
                            .expect("a page holds the tables"),
                        r,
                        sigma,
                        lo: U16Vec2::ZERO,
                        hi: U16Vec2::splat(SIDE as u16),
                    }
                })
                .collect();
            let mut textures = AtlasTextures::new(&gpu.gpu);
            let mut timer = PassTimer::new(gpu);
            let mut belt = UploadStaging::new(&gpu.gpu.device, 1 << 16);
            let handles = gpu.handles();
            // The pool's page, made once: the bake alone is timed.
            timer.time(gpu, |encoder| textures.commit(&handles, encoder, &mut pool));
            assert_eq!(
                textures
                    .page_texture(AtlasFormat::R8, 0)
                    .expect("the tables' page")
                    .width(),
                edge,
                "the tables lie on a page at the edge"
            );
            let mut frame = 0;
            let mut once = || {
                frame += 1;
                let elapsed = timer.time(gpu, |encoder| {
                    let mut ctx = GpuCtx::new(&handles, &mut belt, encoder, frame);
                    tables.bake(&mut ctx, &textures);
                    belt.finish();
                });
                belt.recall();
                elapsed
            };
            once();
            bencher.iter_custom(|iters| (0..iters).map(|_| once()).sum::<Duration>());
        });
    }
    group.finish();
}
