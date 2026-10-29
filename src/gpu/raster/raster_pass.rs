//! The GPU half both raster tenants own, in one type. An icon quad and a glyph quad
//! are the same at the GPU level (a tinted, atlas-sourced rectangle at the raster's
//! pixel size), differing only in what fills the instance buffer. Each tenant gets
//! its own instance, with its own atlas and eviction budget, so a color-icon-heavy
//! frame cannot evict the label's glyphs; the cost is one extra draw on a group
//! mixing icons and text.

use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::pipeline::raster_pipeline::{RasterPipelines, RasterProgram};
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::gpu::raster::raster_atlas::{PackedRaster, RasterAtlas, RasterAtlasConfig};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::paint::raster_image::RasterImage;
use crate::renderer::render_buffer::row_spin::RowSpin;
use glam::Vec2;
use std::fmt::Debug;
use std::hash::Hash;

/// Everything one tenant's pass settles at construction; the pipeline and shader
/// belong to the [`RasterProgram`].
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterPassConfig {
    pub(super) vbuf: &'static str,
    /// The turned quads' buffer label.
    pub(super) spun_vbuf: &'static str,
    pub(super) atlas: RasterAtlasConfig,
    /// Quads the vertex buffer holds before its first growth.
    pub(super) initial_instances: usize,
}

/// What one [`RasterPass::insert_raster`] managed to do with an image. The failures
/// are kept apart because only one is transient.
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
    /// Reported, and this frame fit everything; one more such frame closes the
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
    pub(super) instances: Vec<RasterQuad>,
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
    pub(super) fn new(
        device: &wgpu::Device,
        program: &RasterProgram,
        config: RasterPassConfig,
    ) -> Self {
        Self {
            atlas: RasterAtlas::new(device, program, config.atlas),
            instances: Vec::new(),
            starts: Vec::new(),
            vbuf: DynamicBuffer::vertex(device, config.vbuf, config.initial_instances),
            spun: Vec::new(),
            cuts: Vec::new(),
            cut_starts: Vec::new(),
            // Turned quads are rare: start the buffer small.
            spun_vbuf: DynamicBuffer::vertex(device, config.spun_vbuf, 16),
            stem: config.atlas.label,
            starvation: Starvation::default(),
        }
    }

    /// File a freshly rasterized image under `key` and return its slot. An image
    /// whose extents or bearing overflow [`PackedMetadata`], or that covers no
    /// pixels, takes a slot owning no rectangle, so the key is a hit forever and
    /// the rasterizer runs once.
    pub(super) fn insert_raster(&mut self, key: K, image: RasterImage<'_>) -> Rasterized {
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
        if let Some(idx) = self.atlas.insert(key, raster) {
            Rasterized::Slot(idx)
        } else {
            self.note_atlas_starved();
            Rasterized::AtlasFull
        }
    }

    /// Report the first starved raster of an episode, so a full atlas shows in a
    /// log.
    #[cold]
    fn note_atlas_starved(&mut self) {
        if !self.starvation.note() {
            return;
        }
        tracing::warn!(
            label = self.stem,
            mask_px = self.atlas.side_px(ContentType::Mask),
            color_px = self.atlas.side_px(ContentType::Color),
            live_rasters = self.atlas.cache.len(),
            "atlas is full and cannot grow further; affected batches drop \
             rasters and re-encode every frame until pressure clears",
        );
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

    /// Move the quads emitted from `start` on into the turned buffer, each turned by `spin`. They
    /// keep their place in the batch's paint order.
    pub(super) fn turn_since(&mut self, start: usize, spin: RowSpin) {
        let rotor = Vec2::from_angle(spin.angle);
        let first = self.spun.len() as u32;
        self.spun
            .extend(self.instances.drain(start..).map(|quad| SpunRasterQuad {
                quad,
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
    pub(super) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.vbuf.upload_instances(ctx, &self.instances);
        self.spun_vbuf.upload_instances(ctx, &self.spun);
        self.atlas.flush_pending_uploads(ctx);
    }

    pub(super) fn render_batch<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        batch_idx: usize,
        pipelines: RasterPipelines<'a>,
    ) {
        self.for_each_draw(batch_idx, |draw| {
            let (pipeline, vbuf, span) = match draw {
                RasterDraw::Flat(span) => (pipelines.flat, &self.vbuf.buffer, span),
                RasterDraw::Spun(span) => (pipelines.spun, &self.spun_vbuf.buffer, span),
            };
            binder.pipeline(pass, pipeline, pipelines.layout);
            self.atlas.draw_span(binder, pass, vbuf, span);
        });
    }

    /// Age the atlas against `frame` and drop this frame's quads and batch starts.
    /// Runs for every submit, even one that prepared no batch, or a keep count
    /// would bound only frames that drew something.
    pub(super) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            !self.starts.is_empty() || self.instances.is_empty(),
            "{} quads were emitted with no batch to draw them",
            self.stem,
        );
        self.atlas.advance_to(frame);
        self.instances.clear();
        self.starts.clear();
        self.spun.clear();
        self.cuts.clear();
        self.cut_starts.clear();
        self.starvation.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::raster::raster_pass::Starvation;

    /// The episode's life: reported once on the first starved raster, held open
    /// while starvation continues, closed by the second clean frame. Closing on the
    /// first would re-report a raster that starved again next frame, the per-frame
    /// noise the edge trigger avoids.
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
