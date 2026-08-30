//! Icon render pass — a [`RasterPass`] filled from resvg instead of swash.
//!
//! Everything from the atlas a quad reads to the draw that consumes it
//! lives on `RasterPass`, and the text side owns an instance of the same
//! type. What is here is the icon-shaped half: the loaded sets, the SVG
//! rasterizer, and the prewarm that keeps a filtered icon off the frame
//! path.
//!
//! Rasterization happens here rather than upstream because this is the last
//! point before the draw, and the first at which the icon's true device size
//! is known — the composer has already folded in the display scale and every
//! ancestor transform. Misses rasterize inline, exactly as a glyph miss does.

use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::IconRasterizer;
use crate::icons::icon_registry::IconRegistry;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::IconId;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::raster_atlas::RasterAtlasConfig;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_pass::{
    RasterImage, RasterPass, RasterPassConfig, RasterPassLabels, Rasterized,
};
use crate::renderer::render_buffer::icon::IconDrawRow;
use glam::IVec2;

/// The state one [`IconBackend::prewarm`] pass covered. Both halves matter: a
/// scale change invalidates every raster, and a set loaded afterwards has
/// never been warmed at any scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrewarmMark {
    /// Raster scale, by bits — the value is only ever compared, never used
    /// as a number, so exact equality is what is wanted here.
    scale_bits: u32,
    /// [`IconRegistry::epoch`], not a count of resident sets: a set
    /// released and another loaded leaves the count where it was while
    /// leaving nothing this pass warmed still loaded.
    epoch: u64,
}

#[derive(Debug)]
pub(crate) struct IconBackend {
    pub(super) pass: RasterPass<IconRasterKey>,
    rasterizer: IconRasterizer,
    /// The sets the `Ui` side has loaded. Shared, so an icon loaded on frame
    /// N is rasterizable on frame N.
    icons: IconRegistry,
    /// Raster output, refilled per miss and handed straight to the atlas.
    /// Retained so a steady state that re-rasterizes allocates nothing.
    staging: Vec<u8>,
    /// What [`Self::prewarm`] has already covered, or `None` before it has
    /// run at all.
    warmed: Option<PrewarmMark>,
}

impl IconBackend {
    pub(crate) fn new(device: &wgpu::Device, icons: IconRegistry) -> Self {
        Self {
            pass: RasterPass::new(
                device,
                RasterPassConfig {
                    labels: RasterPassLabels {
                        shader: "palantir.icon.shader",
                        vbuf: "palantir.icon.vbuf",
                        pipeline: "palantir.icon.pipeline",
                        stencil_pipeline: "palantir.icon.pipeline.stencil_test",
                        layout: "palantir.icon.pl",
                    },
                    atlas: RasterAtlasConfig {
                        label: "palantir.icon",
                        // The reverse split from text: a colour icon set is the
                        // expected content here and a tintable one the exception, so
                        // the colour side is the one sized to hold a working set
                        // without an immediate grow chain.
                        initial_mask_px: 256,
                        initial_color_px: 512,
                        // The same 16 MiB as text, and for once the arithmetic agrees
                        // across a 4x difference in bytes per texel: it caps the
                        // colour side at 2048², which holds roughly 450 icons at 48²
                        // — far past any plausible working set — while a larger
                        // budget would only matter under a zoom deep enough that
                        // `MAX_ICON_RASTER_PX` has already bound the raster. A
                        // separate knob because that agreement is a coincidence of the
                        // numbers, not a property of the two tenants.
                        max_bytes: 16 << 20,
                        // 4 MiB reaches 1024² on the colour side, which holds a
                        // working set of a few hundred icons without evicting once.
                        eager_growth_bytes: 4 << 20,
                    },
                    initial_instances: 256,
                },
            ),
            rasterizer: IconRasterizer::default(),
            icons,
            staging: Vec::new(),
            warmed: None,
        }
    }

    /// Rasterize every filtered icon in every loaded set at `scale`, before
    /// the frame path can ask for one.
    ///
    /// An SVG filter costs 10-20x an unfiltered icon of the same size and
    /// grows superlinearly, so a toolbar of them met lazily is a dropped
    /// frame rather than a hitch. Only icons whose survey flagged `filtered`
    /// prewarm: everything else is cheap enough to meet on demand, and
    /// warming it would rasterize icons the session may never draw.
    ///
    /// Re-runs when the scale changes (every raster is invalid) or when a set
    /// is loaded (it has never been warmed).
    pub(super) fn prewarm(&mut self, ctx: &mut GpuCtx<'_>, scale: f32) {
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

    /// Encode one batch of icon rows into instances, rasterizing any that the
    /// atlas does not already hold.
    pub(super) fn prepare_batch(
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
            self.pass.instances.push(RasterQuad {
                pos: [row.origin.x, row.origin.y],
                dim: RasterQuad::dim(slot.size.x, slot.size.y),
                uv_and_kind: RasterQuad::pack_uv(slot.origin.x, slot.origin.y, slot.content)
                    | if row.desaturate {
                        RasterQuad::DESATURATE
                    } else {
                        0
                    },
                color: bytemuck::cast(row.color),
            });
        }
    }

    /// The atlas slab index holding `key`, rasterizing on a miss. `None` when
    /// the icon could not be rasterized, or when the atlas is at its ceiling
    /// with nothing evictable — the second is transient, so the icon simply
    /// misses this frame and is retried on the next.
    fn slot(&mut self, device: &wgpu::Device, key: IconRasterKey) -> Option<u32> {
        if let Some(idx) = self.pass.atlas.touch(&key) {
            return Some(idx);
        }
        let table = self.icons.get(key.icon.set);
        let content = self.rasterizer.rasterize(&table, key, &mut self.staging)?;
        let raster = RasterImage {
            content,
            size: key.size().as_uvec2(),
            // Unlike a glyph, an icon's raster *is* its box, so the
            // composer's origin needs no adjustment.
            bearing: IVec2::ZERO,
            data: &self.staging,
        };
        match self.pass.insert_raster(device, key, raster) {
            Rasterized::Slot(idx) => Some(idx),
            Rasterized::AtlasFull => None,
        }
    }

    /// Unload what a released icon set left behind, then hand the frame
    /// boundary to the pass. Runs for every submit, including one that
    /// prepared no icon batch.
    ///
    /// `frame` is the shared text clock
    /// ([`TextBackend::frame`](crate::renderer::backend::text::TextBackend::frame)),
    /// so both tenants of a `RasterAtlas` age on one clock and a keep
    /// count means the same span in either.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        {
            // Destructured so the drain's closure can hold the two caches
            // mutably while the registry is borrowed — disjoint fields
            // that `self.icons.drain_released(|s| self.…)` could not
            // express.
            let Self {
                icons,
                rasterizer,
                pass,
                ..
            } = self;
            // Both stores key on `IconSetId`, and the registry is about to
            // hand the slot to another set — so this has to happen before
            // any later frame can mint an id that reads as the same slot.
            // One pass over each store however many sets went, which is
            // what keeps a caller that loads a fresh atlas per frame from
            // paying a full walk of both on every one of them.
            icons.drain_released(|sets| {
                rasterizer.forget_sets(sets);
                pass.atlas.forget(|key| !sets.contains(&key.icon.set));
            });
        }
        self.pass.end_frame(frame);
    }
}

#[cfg(test)]
mod tests;
