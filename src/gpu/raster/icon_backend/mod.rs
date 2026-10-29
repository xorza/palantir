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

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
use crate::gpu::pipeline::raster_pipeline::{RasterPipelines, RasterProgram};
use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
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
    pub(crate) fn new(device: &wgpu::Device, program: &RasterProgram, icons: IconRegistry) -> Self {
        Self {
            pass: RasterPass::new(
                device,
                program,
                RasterPassConfig {
                    vbuf: "palantir.icon.vbuf",
                    spun_vbuf: "palantir.icon.spun_vbuf",
                    atlas: RasterAtlasConfig {
                        label: "palantir.icon",
                        // The reverse of text: color icons are the expected content, so the color
                        // side is sized to avoid an immediate grow chain.
                        initial_mask_px: 256,
                        initial_color_px: 512,
                        // The same 16 MiB as text: it caps the color side at 2048^2, far past a
                        // plausible icon working set. A separate knob because the match between
                        // tenants is coincidence, not a property.
                        max_bytes: 16 << 20,
                        // 4 MiB reaches 1024^2 on the color side, enough for a few hundred icons.
                        eager_growth_bytes: 4 << 20,
                    },
                    initial_instances: 256,
                },
            ),
            rasterizer: IconRasterizer::default(),
            icons,
        }
    }

    /// Encode one batch of icon rows into instances, rasterizing atlas misses.
    pub(crate) fn prepare_batch(&mut self, batch_idx: usize, rows: &[IconDrawRow]) {
        self.pass.open_batch(batch_idx);
        for row in rows {
            let Some(idx) = self.slot(row.key) else {
                continue;
            };
            let slot = self.pass.atlas.slots[idx as usize]
                .placement()
                .expect("an icon raster is at least 1x1, so its slot owns a rectangle");
            let mut quad = slot.quad_sized(row.origin, row.size, row.color);
            if row.desaturate {
                quad.uv_and_kind |= RasterQuad::DESATURATE;
            }
            let start = self.pass.instances.len();
            self.pass.instances.push(quad);
            if let Some(spin) = row.spin {
                self.pass.turn_since(start, spin);
            }
        }
    }

    /// The atlas slab index holding `key`, rasterizing on a miss. `None` when the
    /// icon could not be rasterized, or the atlas is at its ceiling with nothing
    /// evictable (transient; retried next frame).
    fn slot(&mut self, key: IconRasterKey) -> Option<u32> {
        if let Some(idx) = self.pass.atlas.touch(&key) {
            return Some(idx);
        }
        let table = self.icons.get(key.icon.set);
        let raster = self.rasterizer.rasterize(&table, key)?;
        match self.pass.insert_raster(key, raster) {
            Rasterized::Slot(idx) => Some(idx),
            Rasterized::AtlasFull => None,
        }
    }

    /// Unload what a released icon set left behind, then hand the frame boundary
    /// to the pass; runs for every submit. `frame` is the shared text clock from
    /// [`TextBackend::end_frame`](crate::gpu::raster::text_backend::TextBackend::end_frame),
    /// so both atlas tenants age on one clock.
    pub(crate) fn end_frame(&mut self, frame: u64) {
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
                pass.atlas.forget(|key| !sets.contains(&key.icon.set));
            });
        }
        self.pass.end_frame(frame);
    }

    /// Upload this frame's quads and any rasters queued for the atlas.
    pub(crate) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.pass.flush(ctx);
    }

    /// Draw the quads `batch_idx` prepared.
    pub(crate) fn render_batch<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        batch_idx: usize,
        pipelines: RasterPipelines<'a>,
    ) {
        self.pass.render_batch(binder, pass, batch_idx, pipelines);
    }
}

#[cfg(test)]
mod tests;
