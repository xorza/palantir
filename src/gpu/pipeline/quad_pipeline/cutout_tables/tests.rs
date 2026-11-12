//! The bake into a mask page against the cutout it tabulates.

use super::*;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacing;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutKey;
use crate::gpu::test_gpu::headless_test_gpu;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::shape::paint::lowered_shadow::ShadowGeometry;
use glam::{U16Vec2, UVec2};

/// What the bake's `CUTOUT_BAKE_NODES` midpoint nodes may err from the cutout's integral
/// (`cutout_bake.wgsl`).
const NODES_ERROR: f64 = 3e-4;

/// What storing a texel in 8 bits may add: D3D's float-to-UNORM rule allows 0.6 of a step, past
/// the half step of rounding to nearest, as Dawn's tests hold every backend to it.
const STORE_ERROR: f64 = 0.6 / 255.0;

/// Midpoint nodes of the reference integral: its error falls as the square of the node count,
/// from the bake's 3e-4 at 24 to under 1e-6 here.
const REFERENCE_NODES: u32 = 512;

/// `Φ`, the standard normal CDF.
fn normal_cdf(v: f64) -> f64 {
    f64::midpoint(1.0, libm::erf(v * std::f64::consts::FRAC_1_SQRT_2))
}

/// `∫ Φ` from −∞ to `v`: `v·Φ(v) + φ(v)`.
fn normal_cdf_integral(v: f64) -> f64 {
    v * normal_cdf(v) + libm::exp(-0.5 * v * v) / (2.0 * std::f64::consts::PI).sqrt()
}

/// `filter_cdf` exact: a Gaussian of `sigma` convolved with the pixel box, its CDF at `u`.
fn filter_cdf(u: f64, sigma: f64) -> f64 {
    let a = f64::from(AA_HALF_WIDTH);
    sigma / (2.0 * a)
        * (normal_cdf_integral((u + a) / sigma) - normal_cdf_integral((u - a) / sigma))
}

/// `filter_pdf` exact: the pixel box's mean of the Gaussian at `u`.
fn filter_pdf(u: f64, sigma: f64) -> f64 {
    let a = f64::from(AA_HALF_WIDTH);
    (normal_cdf((u + a) / sigma) - normal_cdf((u - a) / sigma)) / (2.0 * a)
}

/// A point of a table's square, as `corner_cutout` reflects it.
#[derive(Clone, Copy, Debug)]
struct Q {
    x: f64,
    y: f64,
}

/// `corner_cutout` of `shadow.wgsl` in f64, at [`REFERENCE_NODES`]: the cutout a table's texel
/// at `q` holds, to its integral.
fn corner_cutout(q: Q, r: f64, sigma: f64, reach: f64) -> f64 {
    let outside = |v: f64| v < -reach || v > r + reach;
    if outside(q.x) || outside(q.y) {
        return 0.0;
    }
    let t0 = libm::asin(((q.x - reach) / r).clamp(0.0, 1.0));
    let t1 = libm::asin(((q.x + reach) / r).clamp(0.0, 1.0));
    let step = (t1 - t0) / f64::from(REFERENCE_NODES);
    let top = filter_cdf(r - q.y, sigma);
    (0..REFERENCE_NODES)
        .map(|i| {
            let t = t0 + (f64::from(i) + 0.5) * step;
            let (x, y) = (r * libm::sin(t), r * libm::cos(t));
            filter_pdf(x - q.x, sigma) * (top - filter_cdf(y - q.y, sigma)) * y
        })
        .sum::<f64>()
        * step
}

/// Two tables baked whole, each on a page of its own, hold texel for texel the cutout at the `q`
/// each texel stands for, `i · σ / 10 − reach`, within the nodes' error and an 8-bit store's, and
/// the bake writes nothing outside them. At radius 8 a σ of 16 makes `floor((8 + 2 · 64.5) · 10 /
/// 16) + 2 = 87` texels and takes the series forms; a σ of 2 makes `(8 + 2 · 8.5) · 5 + 2 = 127`
/// and takes the difference forms. On pages of 128², a 16² glyph takes the first page's top shelf,
/// so the 87 lands below it at `(0, 16)`, and the 127 finds no room left there and opens the second
/// page. The deepest cutout of each lies past the bound, so a table left at zero fails it.
#[test]
fn a_baked_table_holds_the_cutout_it_tabulates() {
    let gpu = headless_test_gpu();
    let shader = QuadPipeline::SHADER.module(&gpu);
    let config = PageListConfig {
        first_edge: 128,
        page_edge: 128,
        soft_bytes: 0,
    };
    let mut pool = AtlasPool::with_configs([config; 3], gpu.max_texture_dim().get());
    pool.allocate(AtlasFormat::R8, Tenant::Glyphs, U16Vec2::splat(16))
        .unwrap();
    let mut textures = AtlasTextures::new(&gpu);
    let mut tables = CutoutTables::new(&gpu, &shader, &textures);
    let mut placing = CutoutPlacing {
        placements: &mut tables.placements,
        pool: &mut pool,
    };
    let bakes: Vec<BakeTable> = [(8.0, 16.0, 87), (8.0, 2.0, 127)]
        .into_iter()
        .map(|(r, sigma, side)| BakeTable {
            table: placing.acquire(CutoutKey::new(r, sigma), side).unwrap(),
            r,
            sigma,
            lo: U16Vec2::ZERO,
            hi: U16Vec2::splat(side as u16),
        })
        .collect();
    tables.bakes.clone_from(&bakes);
    gpu.frame(|ctx| {
        textures.commit(ctx.gpu, ctx.encoder, &mut pool);
        tables.pages.follow(ctx.gpu, &textures);
        tables.bake(ctx, &textures);
    });
    let pages =
        [0, 1].map(|page| gpu.read_texture(textures.page_texture(AtlasFormat::R8, page).unwrap()));
    let edge = 128;
    assert_eq!(
        bakes
            .iter()
            .map(|bake| (
                CutoutPlan::table_page(bake.table),
                CutoutPlan::table_origin(bake.table)
            ))
            .collect::<Vec<_>>(),
        [(0, UVec2::new(0, 16)), (1, UVec2::ZERO)]
    );
    for (page, bake) in bakes.iter().enumerate() {
        let texels = &pages[page];
        let origin = CutoutPlan::table_origin(bake.table);
        let (ox, oy) = (origin.x as usize, origin.y as usize);
        let side = CutoutPlan::table_side(bake.table) as usize;
        let (r, sigma) = (f64::from(bake.r), f64::from(bake.sigma));
        let reach = f64::from(ShadowGeometry::REACH_SIGMAS) * sigma + f64::from(AA_HALF_WIDTH);
        let h = sigma / f64::from(CutoutPlan::TEXELS_PER_SIGMA);
        let mut worst = 0.0_f64;
        let mut deepest = 0.0_f64;
        for y in 0..side {
            for x in 0..side {
                let baked = f64::from(texels[(oy + y) * edge + ox + x]) / 255.0;
                let q = Q {
                    x: x as f64 * h - reach,
                    y: y as f64 * h - reach,
                };
                let cutout = corner_cutout(q, r, sigma, reach);
                worst = worst.max((baked - cutout).abs());
                deepest = deepest.max(cutout);
            }
        }
        let bound = NODES_ERROR + STORE_ERROR;
        assert!(deepest > bound, "σ = {sigma}: the deepest cutout {deepest}");
        assert!(
            worst <= bound,
            "σ = {sigma}: a texel {worst:e} from the cutout"
        );
        let outside = texels.iter().enumerate().filter(|&(at, &texel)| {
            let (x, y) = (at % edge, at / edge);
            let inside = (ox..ox + side).contains(&x) && (oy..oy + side).contains(&y);
            !inside && texel != 0
        });
        assert_eq!(outside.count(), 0, "σ = {sigma}: the bake spilled");
    }
}
