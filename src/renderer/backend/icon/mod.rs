//! Icon render pass — the glyph pass with a different atlas.
//!
//! An icon quad and a glyph quad are the same thing at the GPU level: a
//! tinted, atlas-sourced rectangle drawn at exactly the raster's pixel
//! dimensions. So this pass borrows the text pass's
//! [`RasterQuad`](crate::renderer::backend::text::RasterQuad), its
//! shader, and its vertex layout verbatim, and differs only in where the
//! pixels come from: cosmic and swash there, a baked SVG and resvg here.
//!
//! What it does **not** share is the atlas. Icons get their own
//! [`RasterAtlas`] instance — their own textures, bind group, and eviction
//! budget — so a colour-icon-heavy frame cannot evict the glyphs of the label
//! beside it, and so the two can be sized for the content they actually hold.
//! The cost is one extra draw call on a group that mixes icons and text.
//!
//! Rasterization happens here rather than upstream because this is the last
//! point before the draw, and the first at which the icon's true device size
//! is known — the composer has already folded in the display scale and every
//! ancestor transform. Misses rasterize inline, exactly as a glyph miss does.

use crate::icons::icon_atlas::IconId;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::{IconRasterKind, IconRasterizer};
use crate::icons::icon_registry::IconRegistry;
use crate::icons::icon_set::IconRef;
use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::packed_metadata::PackedMetadata;
use crate::renderer::backend::raster_atlas::quad::{self, PARAMS_OFFSET, RasterQuad};
use crate::renderer::backend::raster_atlas::{RasterAtlas, RasterAtlasConfig};
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::icon::IconDrawRow;

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
    atlas: RasterAtlas<IconRasterKey>,
    rasterizer: IconRasterizer,
    /// The sets the `Ui` side has loaded. Shared, so an icon loaded on frame
    /// N is rasterizable on frame N.
    icons: IconRegistry,
    /// Raster output, refilled per miss and handed straight to the atlas.
    /// Retained so a steady state that re-rasterizes allocates nothing.
    staging: Vec<u8>,

    shader: wgpu::ShaderModule,
    /// `[color_atlas_size, mask_atlas_size]`, refreshed only when a side
    instances: Vec<RasterQuad>,
    vbuf: DynamicBuffer<RasterQuad>,
    /// Per-batch slice of [`Self::instances`]; an empty span draws nothing.
    ranges: Vec<Span>,

    /// What [`Self::prewarm`] has already covered, or `None` before it has
    /// run at all.
    warmed: Option<PrewarmMark>,
    /// This atlas's own frame clock. Icons have no shared clock to age
    /// against the way text ages against the shaper's, so eviction counts
    /// submits.
    frame: u64,
}

impl IconBackend {
    pub(crate) fn new(device: &wgpu::Device, icons: IconRegistry) -> Self {
        let shader = quad::shader_module(device, "palantir.icon.shader");
        let atlas = RasterAtlas::new(
            device,
            RasterAtlasConfig {
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
        );
        Self {
            atlas,
            rasterizer: IconRasterizer::default(),
            icons,
            staging: Vec::new(),
            shader,
            instances: Vec::new(),
            vbuf: DynamicBuffer::<RasterQuad>::vertex(device, "palantir icon vbuf", 256),
            ranges: Vec::new(),
            warmed: None,
            frame: 0,
        }
    }

    /// Build this format's pipelines. Same shape as every other pipeline's
    /// `build_variants`; the atlas and its bind group are format-independent
    /// and survive a swapchain format change.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.icon.pipeline",
                stencil_label: "palantir.icon.pipeline.stencil_test",
                layout_label: "palantir.icon.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(self.atlas.bind_group_layout())],
                vertex_buffers: &[Some(quad::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
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
    pub(crate) fn prewarm(&mut self, ctx: &mut GpuCtx<'_>, scale: f32) {
        let mark = PrewarmMark {
            scale_bits: scale.to_bits(),
            epoch: self.icons.epoch(),
        };
        if self.warmed == Some(mark) {
            return;
        }
        self.warmed = Some(mark);
        for (set, atlas) in self.icons.sets() {
            for (i, def) in atlas.icons().iter().enumerate() {
                if !def.filtered {
                    continue;
                }
                let icon = IconRef {
                    set,
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
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        batch_idx: usize,
        rows: &[IconDrawRow],
    ) {
        debug_assert_eq!(
            batch_idx,
            self.ranges.len(),
            "icon batches must be prepared once in contiguous order",
        );
        let start = self.instances.len() as u32;
        for row in rows {
            let Some(idx) = self.slot(ctx.device, row.key) else {
                continue;
            };
            let slot = self.atlas.slots[idx as usize];
            if slot.width == 0 || slot.height == 0 {
                continue;
            }
            self.instances.push(RasterQuad {
                pos: [row.origin.x, row.origin.y],
                dim: RasterQuad::dim(slot.width, slot.height),
                uv_and_kind: quad::pack_uv(slot.x, slot.y, slot.content)
                    | if row.desaturate { quad::DESATURATE } else { 0 },
                color: bytemuck::cast(row.color),
            });
        }
        self.ranges
            .push(Span::new(start, self.instances.len() as u32 - start));
    }

    /// The atlas slab index holding `key`, rasterizing on a miss. `None` when
    /// the icon could not be rasterized, or when the atlas is at its ceiling
    /// with nothing evictable — the second is transient, so the icon simply
    /// misses this frame and is retried on the next.
    fn slot(&mut self, device: &wgpu::Device, key: IconRasterKey) -> Option<u32> {
        if let Some(idx) = self.atlas.touch(&key) {
            return Some(idx);
        }
        let atlas = self.icons.get(key.icon.set);
        let kind = self.rasterizer.rasterize(&atlas, key, &mut self.staging)?;
        let content = match kind {
            IconRasterKind::Mask => ContentType::Mask,
            IconRasterKind::Color => ContentType::Color,
        };
        let metadata = PackedMetadata::new(
            u32::from(key.size.x),
            u32::from(key.size.y),
            // No bearing: unlike a glyph, an icon's raster *is* its box, so
            // the composer's origin needs no adjustment.
            0,
            0,
        )?;
        if metadata.is_empty() {
            return Some(self.atlas.insert_unallocated(key, content, metadata));
        }
        self.atlas
            .insert(device, key, content, metadata, &self.staging)
    }

    /// Upload this frame's instances in one belt write, then drain the atlas's
    /// queued uploads onto the renderer's encoder — so the pixels land in the
    /// same submit as the draws that read them.
    pub(crate) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.vbuf.upload_instances(ctx, &self.instances);
        self.atlas.flush_pending_uploads(ctx);
    }

    pub(crate) fn render_batch<'a>(
        &'a self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        viewport: &ViewportPush,
    ) {
        let &span = self
            .ranges
            .get(batch_idx)
            .expect("render schedule referenced an unprepared icon batch");
        if span.len == 0 {
            return;
        }
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_bind_group(0, self.atlas.bind_group(), &[]);
        // Both halves of the shared immediate region, for the same reason the
        // text pass writes both: icons can be the first pipeline bound in a
        // pass, so no earlier step is guaranteed to have pushed the viewport.
        viewport.push_into(pass);
        pass.set_immediates(PARAMS_OFFSET, bytemuck::bytes_of(&self.atlas.atlas_px()));
        pass.set_vertex_buffer(0, self.vbuf.buffer.slice(..));
        pass.draw(0..4, span.start..span.start + span.len);
    }

    /// Frame teardown, run for every submit — including one that prepared no
    /// icon batch, so a frame whose damage missed every icon still ages the
    /// atlas and still unloads what a dropped set left behind.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        {
            // Destructured so the drain's closure can hold the two caches
            // mutably while the registry is borrowed — disjoint fields
            // that `self.icons.drain_released(|s| self.…)` could not
            // express.
            let Self {
                icons,
                rasterizer,
                atlas,
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
                atlas.forget(|key| !sets.contains(&key.icon.set));
            });
        }
        self.atlas.end_frame(self.frame);
        self.instances.clear();
        self.ranges.clear();
    }
}

#[cfg(test)]
mod tests;
