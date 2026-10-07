//! Icon render pass: a [`RasterPass`] filled from resvg instead of swash.
//!
//! The atlas-to-draw machinery lives on `RasterPass`, shared with text. Here:
//! the loaded sets, the SVG rasterizer, and the prewarm that keeps filtered
//! icons off the frame path.
//!
//! Rasterization is here because it is the first point where the icon's true
//! device size is known (display scale and ancestor transforms folded in).
//! Misses rasterize inline, like a glyph miss.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::raster::raster_pass::{RasterPass, RasterPassConfig, Rasterized};
use crate::gpu::raster::raster_program::RasterProgram;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::IconRasterizer;
use crate::icons::icon_registry::IconRegistry;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::IconId;
use crate::renderer::render_buffer::icon::IconDrawRow;

/// What one [`IconBackend::prewarm`] pass covered: a scale change invalidates
/// every raster, and a later-loaded set was never warmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrewarmMark {
    /// Raster scale, by bits: only compared, so exact equality is wanted.
    scale_bits: u32,
    /// [`IconRegistry::epoch`], not a set count: releasing one set and loading
    /// another keeps the count but leaves nothing warmed.
    epoch: u64,
}

#[derive(Debug)]
pub(crate) struct IconBackend {
    pass: RasterPass<IconRasterKey>,
    rasterizer: IconRasterizer,
    /// The sets the `Ui` side has loaded; shared so a set loaded on frame N
    /// rasterizes on frame N.
    icons: IconRegistry,
    /// What [`Self::prewarm`] covered, or `None` before the first run.
    warmed: Option<PrewarmMark>,
}

impl IconBackend {
    pub(crate) fn new(device: &wgpu::Device, program: &RasterProgram, icons: IconRegistry) -> Self {
        Self {
            pass: RasterPass::new(
                device,
                program,
                RasterPassConfig {
                    vbuf: "palantir.icon.vbuf",
                    atlas: RasterAtlasConfig {
                        label: "palantir.icon",
                        // The reverse of text: colour icons are the expected content, so the colour
                        // side is sized to avoid an immediate grow chain.
                        initial_mask_px: 256,
                        initial_color_px: 512,
                        // The same 16 MiB as text: it caps the colour side at 2048^2, far past a
                        // plausible icon working set. A separate knob because the match between
                        // tenants is coincidence, not a property.
                        max_bytes: 16 << 20,
                        // 4 MiB reaches 1024^2 on the colour side, enough for a few hundred icons.
                        eager_growth_bytes: 4 << 20,
                    },
                    initial_instances: 256,
                },
            ),
            rasterizer: IconRasterizer::default(),
            icons,
            warmed: None,
        }
    }

    /// Rasterize every filtered icon in every loaded set at `scale` before the
    /// frame path asks for one.
    ///
    /// An SVG filter costs far more than an unfiltered icon and grows
    /// superlinearly, so lazily meeting a toolbar of them drops a frame. Only
    /// icons flagged `filtered` prewarm; the rest are cheap on demand.
    ///
    /// Re-runs when the scale changes or a set is loaded.
    pub(crate) fn prewarm(&mut self, ctx: &mut GpuCtx<'_>, scale: f32) {
        let mark = PrewarmMark {
            scale_bits: scale.to_bits(),
            epoch: self.icons.epoch(),
        };
        if self.warmed == Some(mark) {
            return;
        }
        self.warmed = Some(mark);
        for slot in 0..self.icons.slot_count() {
            let Some(set) = self.icons.resident(slot) else {
                continue;
            };
            for (i, def) in set.table.icons().iter().enumerate() {
                if !def.filtered {
                    continue;
                }
                let icon = IconRef {
                    set: set.id,
                    icon: IconId(i as u16),
                };
                self.slot(
                    ctx.device,
                    IconRasterKey::for_box(icon, def.view_box * scale),
                );
            }
        }
    }

    /// Encode one batch of icon rows into instances, rasterizing atlas misses.
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        batch_idx: usize,
        rows: &[IconDrawRow],
    ) {
        self.pass.open_batch(batch_idx);
        for row in rows {
            let Some(idx) = self.slot(ctx.device, row.key) else {
                continue;
            };
            let slot = self.pass.atlas.slots[idx as usize]
                .placement
                .expect("an icon raster is at least 1x1, so its slot owns a rectangle");
            let mut quad = slot.quad_sized(row.origin, row.size, row.color);
            if row.desaturate {
                quad.uv_and_kind |= RasterQuad::DESATURATE;
            }
            self.pass.instances.push(quad);
        }
    }

    /// The atlas slab index holding `key`, rasterizing on a miss. `None` when the
    /// icon could not be rasterized, or the atlas is at its ceiling with nothing
    /// evictable (transient; retried next frame).
    fn slot(&mut self, device: &wgpu::Device, key: IconRasterKey) -> Option<u32> {
        if let Some(idx) = self.pass.atlas.touch(&key) {
            return Some(idx);
        }
        let table = self.icons.get(key.icon.set);
        let raster = self.rasterizer.rasterize(&table, key)?;
        match self.pass.insert_raster(device, key, raster) {
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
    pub(crate) fn render_batch<'a>(&'a self, batch_idx: usize, pass: &mut wgpu::RenderPass<'a>) {
        self.pass.render_batch(batch_idx, pass);
    }
}

#[cfg(test)]
mod tests;
