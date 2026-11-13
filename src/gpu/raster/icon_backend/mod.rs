//! Icon render pass: a [`RasterPass`] filled from resvg instead of swash.
//!
//! The atlas-to-draw machinery lives on `RasterPass`, shared with text. Here:
//! the loaded sets and the SVG rasterizer.
//!
//! Rasterization is here because it is the first point where the icon's true
//! device size is known (display scale and ancestor transforms folded in).
//! Misses rasterize inline, like a glyph miss: a raster warmed before its draw
//! would have to guess that size, and one deferred past it would leave the
//! frame without the icon.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::RasterPipelinePair;
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::gpu::raster::raster_pass::{RasterPass, RasterPassConfig, Rasterized};
use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::IconRasterizer;
use crate::icons::icon_registry::IconRegistry;
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;

#[derive(Debug)]
pub(crate) struct IconBackend {
    pass: RasterPass<IconRasterKey>,
    rasterizer: IconRasterizer,
    /// The sets the `Ui` side has loaded; shared so a set loaded on frame N
    /// rasterizes on frame N.
    icons: IconRegistry,
}

impl IconBackend {
    pub(crate) fn new(gpu: &Gpu, icons: IconRegistry) -> Self {
        Self {
            pass: RasterPass::new(
                gpu,
                RasterPassConfig {
                    label: "palantir.icon",
                    tenant: Tenant::Icons,
                    vbuf: "palantir.icon.vbuf",
                    spun_vbuf: "palantir.icon.spun_vbuf",
                    initial_instances: 256,
                },
            ),
            rasterizer: IconRasterizer::default(),
            icons,
        }
    }

    /// Encode one batch of icon rows into instances, rasterizing atlas misses.
    pub(crate) fn prepare_batch(
        &mut self,
        pool: &mut AtlasPool,
        batch_idx: usize,
        rows: &[IconDrawRow],
    ) {
        self.pass.open_batch(batch_idx);
        for row in rows {
            let Some(idx) = self.slot(pool, row.key) else {
                continue;
            };
            // A raster past a page takes a slot that draws nothing, as a glyph's does.
            let Some(slot) = self.pass.atlas.placement(pool, idx) else {
                continue;
            };
            let mut quad = RasterQuad {
                clip: row.clip,
                ..slot.quad_sized(row.origin, row.size, row.color)
            };
            if row.desaturate {
                quad.uv_and_kind |= RasterQuad::DESATURATE;
            }
            let start = self.pass.instance_count();
            self.pass.push(quad);
            if let Some(spin) = row.spin {
                self.pass.turn_since(start, spin, row.clip);
            }
        }
    }

    /// The atlas slab index holding `key`, rasterizing on a miss. `None` when the
    /// icon could not be rasterized, or the atlas is at its ceiling with nothing
    /// evictable (transient; retried next frame).
    fn slot(&mut self, pool: &mut AtlasPool, key: IconRasterKey) -> Option<u32> {
        if let Some(idx) = self.pass.atlas.touch(pool, &key) {
            return Some(idx);
        }
        let table = self.icons.get(key.icon.set);
        let raster = self.rasterizer.rasterize(&table, key)?;
        match self.pass.insert_raster(pool, key, raster) {
            Rasterized::Slot(idx) => Some(idx),
            Rasterized::AtlasFull => None,
        }
    }

    /// Unload what a released icon set left behind, then hand the frame boundary
    /// to the pass; runs for every submit. `frame` is the shared text clock from
    /// [`TextBackend::end_frame`](crate::gpu::raster::text_backend::TextBackend::end_frame),
    /// so both atlas tenants age on one clock.
    pub(crate) fn end_frame(&mut self, pool: &mut AtlasPool, frame: u64) {
        {
            // Destructured so the drain closure can borrow the two caches mutably while
            // the registry is borrowed.
            let Self {
                icons,
                rasterizer,
                pass,
                ..
            } = self;
            // Both stores key on `IconSetId`, and the registry is about to reuse the
            // slot, so this must precede any later frame minting a colliding id. One pass
            // per store however many sets were released.
            icons.drain_released(|sets| {
                rasterizer.forget_sets(sets);
                pass.atlas.forget(pool, |key| !sets.contains(&key.icon.set));
            });
        }
        self.pass.end_frame(pool, frame);
    }

    /// Upload this frame's quads and any rasters queued for the pool's pages, after its commit.
    pub(crate) fn flush(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        self.pass.flush(ctx, textures);
    }

    /// Draw the quads `batch_idx` prepared.
    pub(crate) fn render_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        batch_idx: usize,
        pipelines: RasterPipelinePair<'a>,
    ) {
        self.pass.render_batch(rec, batch_idx, pipelines);
    }
}

#[cfg(test)]
mod tests;
