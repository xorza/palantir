//! The GPU half both raster tenants own, in one type. An icon quad and a glyph quad
//! are the same at the GPU level (a tinted, atlas-sourced rectangle at the raster's
//! pixel size), differing only in what fills the instance buffer. Each tenant gets
//! its own instance and its own keys, over the one atlas pool both share, and both
//! draw through the one atlas group (`src/gpu/AGENTS.md`, Atlases): an
//! eviction takes only what no frame drew this frame or the last, of either.

use crate::common::span::Span;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::pipeline::raster_pipeline::{ATLAS_GROUP, RasterPipelinePair};
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::gpu::raster::raster_atlas::{PackedRaster, RasterAtlas};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::libm_ext::LibmVec2;
use crate::primitives::paint::raster_image::RasterImage;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::row_spin::RowSpin;
use glam::Vec2;
use std::fmt::Debug;
use std::hash::Hash;

/// Everything one tenant's pass settles at construction; the pipeline and shader
/// belong to the [`RasterPipeline`](crate::gpu::pipeline::raster_pipeline::RasterPipeline).
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterPassConfig {
    /// Names the tenant in logs and GPU labels.
    pub(super) label: &'static str,
    pub(super) tenant: Tenant,
    pub(super) vbuf: &'static str,
    /// The turned quads' buffer label.
    pub(super) spun_vbuf: &'static str,
    /// Quads the vertex buffer holds before its first growth.
    pub(super) initial_instances: usize,
}

/// What one [`RasterPass::insert_raster`] managed to do with an image: a slot, one that draws
/// nothing for an image no frame can place, or the one failure a later frame clears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rasterized {
    Slot(u32),
    /// The atlas is at the device maximum with no evictable rectangle; the image is
    /// missing *this frame only*.
    AtlasFull,
}

/// The life of one atlas-starvation episode. Starvation is not corruption (the
/// image re-encodes next frame) but it is silent slowness with a visible hole, so
/// it is edge-triggered: it recurs per raster per frame. Three named states rather
/// than two bools, which admit a meaningless fourth combination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Starvation {
    #[default]
    Clear,
    Open,
    /// Reported, and the last starved frame has ended; one frame without starvation closes the
    /// episode so a recurrence is reported again.
    Settling,
}

impl Starvation {
    /// Record a starved raster, answering whether it is the first of its episode.
    fn note(&mut self) -> bool {
        let first = *self == Self::Clear;
        *self = Self::Open;
        first
    }

    const fn end_frame(&mut self) {
        *self = match self {
            Self::Open => Self::Settling,
            Self::Clear | Self::Settling => Self::Clear,
        };
    }
}

/// One draw of a batch: unturned quads from [`RasterPass::instances`], or turned ones from
/// [`RasterPass::spun`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RasterDraw {
    Flat(Span),
    Spun(Span),
}

/// The turned quads `spun` draw before the unturned quad at `at`, where they were emitted.
#[derive(Clone, Copy, Debug)]
struct SpunCut {
    at: u32,
    spun: Span,
}

#[derive(Debug)]
pub(super) struct RasterPass<K> {
    pub(super) atlas: RasterAtlas<K>,
    instances: Vec<RasterQuad>,
    /// Where each batch's quads start in [`Self::instances`]; the next entry (or
    /// the instance count) is where they end.
    starts: Vec<u32>,
    vbuf: DynamicBuffer<RasterQuad>,
    /// The quads a paint animation turns, moved out of [`Self::instances`] by
    /// [`Self::turn_since`].
    pub(super) spun: Vec<SpunRasterQuad>,
    /// Where each run of [`Self::spun`] draws among the unturned quads, in emission order.
    cuts: Vec<SpunCut>,
    /// Where each batch's cuts start in [`Self::cuts`], as [`Self::starts`] holds quads.
    cut_starts: Vec<u32>,
    spun_vbuf: DynamicBuffer<SpunRasterQuad>,
    stem: &'static str,
    starvation: Starvation,
}

impl<K: Copy + Eq + Hash + Debug> RasterPass<K> {
    pub(super) fn new(gpu: &Gpu, config: RasterPassConfig) -> Self {
        Self {
            atlas: RasterAtlas::new(config.tenant, config.label),
            instances: Vec::new(),
            starts: Vec::new(),
            vbuf: DynamicBuffer::vertex(gpu, config.vbuf, config.initial_instances),
            spun: Vec::new(),
            cuts: Vec::new(),
            cut_starts: Vec::new(),
            // Turned quads are rare: start the buffer small.
            spun_vbuf: DynamicBuffer::vertex(gpu, config.spun_vbuf, 16),
            stem: config.label,
            starvation: Starvation::default(),
        }
    }

    /// File a freshly rasterized image under `key` and return its slot. An image
    /// whose extents or bearing overflow [`PackedMetadata`], that covers no
    /// pixels, or that passes a page takes a slot owning no rectangle, so the key
    /// is a hit forever and the rasterizer runs once.
    pub(super) fn insert_raster(
        &mut self,
        pool: &mut AtlasPool,
        key: K,
        image: RasterImage<'_>,
    ) -> Rasterized {
        let Some(metadata) = PackedMetadata::new(image.size, image.bearing) else {
            tracing::warn!(
                ?key,
                width = image.size.x,
                height = image.size.y,
                left = image.bearing.x,
                top = image.bearing.y,
                label = self.stem,
                "skipping raster outside packed atlas metadata range",
            );
            return Rasterized::Slot(self.atlas.insert_unallocated(key));
        };
        if metadata.is_empty() {
            return Rasterized::Slot(self.atlas.insert_unallocated(key));
        }
        let raster = PackedRaster {
            metadata,
            content: image.content,
            pixels: image.data,
        };
        if let Some(idx) = self.atlas.insert(pool, key, raster) {
            Rasterized::Slot(idx)
        } else {
            self.note_atlas_starved(pool);
            Rasterized::AtlasFull
        }
    }

    /// Report the first starved raster of an episode, so a full atlas shows in a
    /// log.
    #[cold]
    fn note_atlas_starved(&mut self, pool: &AtlasPool) {
        if !self.starvation.note() {
            return;
        }
        tracing::warn!(
            label = self.stem,
            mask_bytes = pool.list(AtlasFormat::R8).bytes(),
            color_bytes = pool.list(AtlasFormat::Rgba8).bytes(),
            live_rasters = self.atlas.cached_len(),
            "atlas is full and cannot grow further; affected batches drop \
             rasters and re-encode every frame until pressure clears",
        );
    }

    /// Add `quad` to the batch open.
    pub(super) fn push(&mut self, quad: RasterQuad) {
        self.instances.push(quad);
    }

    /// Take back the quads pushed since `start`, an [`Self::instance_count`] of this batch.
    pub(super) fn truncate(&mut self, start: usize) {
        debug_assert!(
            start <= self.instances.len(),
            "a truncation past the quads pushed"
        );
        self.instances.truncate(start);
    }

    /// The quads pushed this frame, where the next lands: the start a
    /// [`Self::turn_since`] takes.
    pub(super) const fn instance_count(&self) -> usize {
        self.instances.len()
    }

    pub(super) fn open_batch(&mut self, batch_idx: usize) {
        debug_assert_eq!(
            batch_idx,
            self.starts.len(),
            "{} batches must be prepared once in contiguous order",
            self.stem,
        );
        self.starts.push(self.instances.len() as u32);
        self.cut_starts.push(self.cuts.len() as u32);
    }

    /// Clip the quads emitted from `start` on to `bounds`, each to its part inside them
    /// ([`RasterQuad::clipped`]), dropping one wholly outside, so a run a clip cuts needs no
    /// scissor of its own; each kept one takes the rounded clips of `chain`, as the cached quads
    /// of a run hold none.
    pub(super) fn clip_since(&mut self, start: usize, bounds: URect, chain: ClipChain) {
        let min = bounds.min.as_ivec2();
        let max = (bounds.min + bounds.size).as_ivec2();
        let mut kept = start;
        for at in start..self.instances.len() {
            if let Some(quad) = self.instances[at].clipped(min, max) {
                self.instances[kept] = RasterQuad {
                    clip: chain,
                    ..quad
                };
                kept += 1;
            }
        }
        self.instances.truncate(kept);
    }

    /// Move the quads emitted from `start` on into the turned buffer, each turned by `spin` and
    /// cut by the rounded clips of `chain`. They keep their place in the batch's paint order.
    pub(super) fn turn_since(&mut self, start: usize, spin: RowSpin, chain: ClipChain) {
        let rotor = Vec2::libm_from_angle(spin.angle);
        let first = self.spun.len() as u32;
        self.spun
            .extend(self.instances.drain(start..).map(|quad| SpunRasterQuad {
                quad: RasterQuad {
                    clip: chain,
                    ..quad
                },
                pivot: spin.pivot,
                rotor,
            }));
        let len = self.spun.len() as u32 - first;
        if len == 0 {
            return;
        }
        let at = start as u32;
        let batch_cuts = *self
            .cut_starts
            .last()
            .expect("quads are emitted into an open batch") as usize;
        let in_batch = self.cuts.len() > batch_cuts;
        match self.cuts.last_mut() {
            // Turned runs back to back draw as one, as `spun` holds them contiguously.
            Some(cut) if in_batch && cut.at == at => cut.spun.len += len,
            _ => self.cuts.push(SpunCut {
                at,
                spun: Span::new(first, len),
            }),
        }
    }

    pub(super) fn batch_span(&self, batch_idx: usize) -> Span {
        Self::span_in(&self.starts, self.instances.len(), batch_idx, self.stem)
    }

    /// Batch `batch_idx`'s draws in the order its quads were emitted, none of them empty.
    pub(super) fn for_each_draw(&self, batch_idx: usize, mut draw: impl FnMut(RasterDraw)) {
        let flat = self.batch_span(batch_idx);
        let cuts = Self::span_in(&self.cut_starts, self.cuts.len(), batch_idx, self.stem);
        let mut at = flat.start;
        for cut in &self.cuts[cuts.range()] {
            if cut.at > at {
                draw(RasterDraw::Flat(Span::new(at, cut.at - at)));
            }
            draw(RasterDraw::Spun(cut.spun));
            at = cut.at;
        }
        let end = flat.start + flat.len;
        if end > at {
            draw(RasterDraw::Flat(Span::new(at, end - at)));
        }
    }

    /// Batch `batch_idx`'s span of a buffer of `len` quads whose batch starts are `starts`.
    fn span_in(starts: &[u32], len: usize, batch_idx: usize, stem: &str) -> Span {
        let Some(&start) = starts.get(batch_idx) else {
            panic!("render schedule referenced an unprepared {stem} batch");
        };
        let end = starts.get(batch_idx + 1).copied().unwrap_or(len as u32);
        Span::new(start, end - start)
    }

    /// Upload this frame's quads in one belt write, then drain the atlas's queued
    /// uploads onto the renderer's encoder. Called once per frame after every batch
    /// is prepared and before any pass draws, so pixels land in the same submit as
    /// the draws reading them.
    pub(super) fn flush(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        self.vbuf.upload_instances(ctx, &self.instances);
        self.spun_vbuf.upload_instances(ctx, &self.spun);
        self.atlas.flush_pending_uploads(ctx, textures);
    }

    /// Draw batch `batch_idx`'s quads through `pipelines`, reading the pool's pages through the
    /// atlas group in `pipelines`.
    pub(super) fn render_batch<'a>(
        &'a self,
        rec: &mut PassRecorder<'a>,
        batch_idx: usize,
        pipelines: RasterPipelinePair<'a>,
    ) {
        self.for_each_draw(batch_idx, |draw| {
            let (pipeline, vbuf, span) = match draw {
                RasterDraw::Flat(span) => (pipelines.flat, &self.vbuf.buffer, span),
                RasterDraw::Spun(span) => (pipelines.spun, &self.spun_vbuf.buffer, span),
            };
            rec.pipeline(pipeline);
            rec.group(ATLAS_GROUP, pipelines.atlas);
            rec.vertex(0, vbuf);
            rec.draw(0..4, span.start..span.start + span.len);
        });
    }

    /// Age the atlas against `frame` and drop this frame's quads and batch starts.
    /// Runs for every submit, even one that prepared no batch, or a keep count
    /// would bound only frames that drew something.
    pub(super) fn end_frame(&mut self, pool: &mut AtlasPool, frame: u64) {
        debug_assert!(
            !self.starts.is_empty() || self.instances.is_empty(),
            "{} quads were emitted with no batch to draw them",
            self.stem,
        );
        self.atlas.end_frame(pool, frame);
        self.instances.clear();
        self.starts.clear();
        self.spun.clear();
        self.cuts.clear();
        self.cut_starts.clear();
        self.starvation.end_frame();
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::raster_pipeline::raster_quad::RasterQuad;
    use crate::gpu::raster::raster_pass::RasterPass;

    impl<K> RasterPass<K> {
        /// The unturned quads pushed this frame.
        pub(crate) fn instances(&self) -> &[RasterQuad] {
            &self.instances
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::raster::raster_pass::Starvation;

    /// The episode's life: reported once on the first starved raster, held open while starvation
    /// continues, closed by the first frame after the last starved one that starves nothing.
    /// Closing as the starved frame ends would re-report a raster that starved again next frame,
    /// the per-frame noise the edge trigger avoids.
    #[test]
    fn a_starvation_episode_reports_once_and_closes_one_clean_frame_later() {
        let mut s = Starvation::default();
        assert!(
            s.note(),
            "the first starved raster of an episode is reported"
        );
        assert!(!s.note(), "later rasters on the same frame are not");

        s.end_frame();
        assert_eq!(s, Starvation::Settling);
        assert!(
            !s.note(),
            "starving again while settling is the same episode",
        );

        s.end_frame();
        assert_eq!(s, Starvation::Settling);
        s.end_frame();
        assert_eq!(s, Starvation::Clear);
        assert!(s.note(), "a fresh episode is reported again");
    }
}
