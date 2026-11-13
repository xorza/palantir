//! The GPU half both raster tenants own, in one type. An icon quad and a glyph quad
//! are the same at the GPU level (a tinted, atlas-sourced rectangle at the raster's
//! pixel size), differing only in what fills the instance buffer. Each tenant gets
//! its own instance and its own keys, over the one atlas pool both share, and both
//! draw through the one atlas group (`src/gpu/AGENTS.md`, Atlases): an
//! eviction takes only what no frame drew this frame or the last, of either.

use crate::common::span::Span;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::Placement;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::debug_marker;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::pipeline::raster_pipeline::{ATLAS_GROUP, RasterPipelinePair};
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::gpu::raster::raster_atlas::{Found, RasterAtlas, format_of};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::{GpuCtx, RowCopy};
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::libm_ext::LibmVec2;
use crate::primitives::paint::raster_image::RasterImage;
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::row_spin::RowSpin;
use glam::Vec2;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Range;

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

/// What [`RasterPass::file`] made of a rasterized image: an entry, or the one failure a later
/// frame clears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filed {
    Found(Found),
    /// The atlas is at the device maximum with no evictable rectangle; the image is missing
    /// *this frame only*.
    Full,
}

/// One raster's pixels queued for its rectangle until the pool's commit: its list and placement,
/// and its rows in [`RasterUploads::pixels`].
#[derive(Clone, Debug)]
struct PendingCopy {
    format: AtlasFormat,
    placement: Placement,
    pixels: Range<usize>,
}

/// The rasters a tenant filed this frame, waiting for the pool's commit to make their pages.
#[derive(Debug)]
struct RasterUploads {
    /// GPU debug label of the upload, built once: a per-frame `format!` would allocate.
    label: String,
    /// Raster pixels, rows as packed: [`GpuCtx::copy_rows`] pads them to the copy pitch once, in
    /// the belt's mapped staging.
    pixels: Vec<u8>,
    copies: Vec<PendingCopy>,
}

impl RasterUploads {
    fn new(label: &str) -> Self {
        Self {
            label: format!("{label} atlas batch upload"),
            pixels: Vec::new(),
            copies: Vec::new(),
        }
    }

    /// Queue `pixels`, rows with no slack, for `placement` on `format`'s pages.
    fn push(&mut self, format: AtlasFormat, placement: Placement, pixels: &[u8]) {
        debug_assert_eq!(
            pixels.len(),
            placement.texels() as usize * format.texel_bytes() as usize,
            "a raster's bytes must be its rows with no slack",
        );
        let start = self.pixels.len();
        self.pixels.extend_from_slice(pixels);
        self.copies.push(PendingCopy {
            format,
            placement,
            pixels: start..self.pixels.len(),
        });
    }

    /// Drain the queue into `textures`' pages, once per frame after the pool's commit and before
    /// any pass draws, through [`GpuCtx::copy_rows`]: the bytes stage once, and no buffer of this
    /// tenant's own holds the peak frame's worth. A raster's placement is the one it was filed at,
    /// as no decay or purge runs between a submit's inserts and its flush.
    fn flush(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        if self.copies.is_empty() {
            return;
        }
        let pixels = &self.pixels;
        debug_marker::push(ctx.encoder, &self.label);
        ctx.copy_rows(self.copies.iter().map(|copy| {
            RowCopy {
                texture: textures
                    .page_texture(copy.format, usize::from(copy.placement.page))
                    .expect("a raster's page was committed before its upload"),
                origin: copy.placement.origin.as_uvec2(),
                size: copy.placement.size.as_uvec2(),
                rows: &pixels[copy.pixels.clone()],
            }
        }));
        debug_marker::pop(ctx.encoder);
        self.pixels.clear();
        self.copies.clear();
    }
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
    uploads: RasterUploads,
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
            atlas: RasterAtlas::new(config.tenant),
            uploads: RasterUploads::new(config.label),
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

    /// File `image`, rasterized for `key`, which [`RasterAtlas::find`] missed: on a page, its
    /// pixels queued for the commit, or as drawing nothing where no frame could place it, so the
    /// key is a hit and its rasterizer runs once. Nothing to draw: no image, one that covers no
    /// pixels, extents or a bearing past what a slot carries, or wider than a page, which
    /// [`AtlasCounters::oversized`](crate::gpu::raster::raster_atlas::counters::AtlasCounters)
    /// counts. [`Filed::Full`] where the pool is full this frame.
    pub(super) fn file(
        &mut self,
        pool: &mut AtlasPool,
        key: K,
        image: Option<RasterImage<'_>>,
    ) -> Filed {
        let frame = pool.frame();
        let Some(image) = image else {
            self.atlas.insert_unplaced(key, frame);
            return Filed::Found(Found::Nothing);
        };
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
            self.atlas.insert_unplaced(key, frame);
            return Filed::Found(Found::Nothing);
        };
        if metadata.is_empty() {
            self.atlas.insert_unplaced(key, frame);
            return Filed::Found(Found::Nothing);
        }
        let format = format_of(image.content);
        match pool.allocate(format, self.atlas.tenant(), metadata.size.as_uvec2()) {
            Ok(allocation) => {
                self.uploads.push(format, allocation.placement, image.data);
                Filed::Found(Found::Raster(self.atlas.insert_placed(
                    key,
                    allocation,
                    metadata.bearing,
                )))
            }
            Err(AtlasError::Oversized { size, edge }) => {
                self.atlas.counters.oversized.bump();
                tracing::warn!(
                    ?key,
                    width = size.x,
                    height = size.y,
                    edge,
                    label = self.stem,
                    "skipping raster past the atlas page edge",
                );
                self.atlas.insert_unplaced(key, frame);
                Filed::Found(Found::Nothing)
            }
            Err(AtlasError::Full) => {
                self.note_atlas_starved(pool);
                Filed::Full
            }
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
        self.uploads.flush(ctx, textures);
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

    /// Retire the atlas's non-drawing entries due at display frame `frame` and drop this frame's
    /// quads and batch starts. Runs for every submit, even one that prepared no batch, or a keep
    /// count would bound only frames that drew something.
    pub(super) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            !self.starts.is_empty() || self.instances.is_empty(),
            "{} quads were emitted with no batch to draw them",
            self.stem,
        );
        self.atlas.expire(frame);
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
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::atlas::atlas_format::AtlasFormat;
    use crate::gpu::atlas::tenant::Tenant;
    use crate::gpu::raster::raster_atlas::Found;
    use crate::gpu::raster::raster_pass::{Filed, RasterPass, RasterPassConfig, Starvation};
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::primitives::paint::content_type::ContentType;
    use crate::primitives::paint::raster_image::RasterImage;
    use glam::{I16Vec2, IVec2, U16Vec2, UVec2};

    /// An image files on a page of its content's list, its pixels queued for that list, or as
    /// drawing nothing where no frame could place it, so its key is a hit: no image, one of no
    /// pixels, one past what a slot carries, one past a page, which counts. A pool full of what
    /// this frame drew files nothing, and queues nothing. On pages of 32², four at most.
    #[test]
    fn an_image_files_on_a_page_or_as_drawing_nothing() {
        let gpu = headless_test_gpu();
        let mut pass = RasterPass::<u16>::new(
            &gpu,
            RasterPassConfig {
                label: "test",
                tenant: Tenant::Glyphs,
                vbuf: "test.vbuf",
                spun_vbuf: "test.spun_vbuf",
                initial_instances: 4,
            },
        );
        let mut pool = AtlasPool::uniform(32, 32, 0);
        let image = |content, size: UVec2, data| RasterImage {
            content,
            size,
            bearing: IVec2::new(1, 2),
            data,
        };
        let raster = |filed| match filed {
            Filed::Found(Found::Raster(placement)) => placement,
            other => panic!("{other:?} files no raster"),
        };
        let (mask, color) = (vec![0; 10 * 12], vec![0; 4 * 4 * 4]);
        let m = raster(pass.file(
            &mut pool,
            1,
            Some(image(ContentType::Mask, UVec2::new(10, 12), &mask)),
        ));
        assert_eq!(
            (m.page, m.origin, m.size, m.bearing, m.content()),
            (
                0,
                U16Vec2::ZERO,
                U16Vec2::new(10, 12),
                I16Vec2::new(1, 2),
                ContentType::Mask
            )
        );
        let c = raster(pass.file(
            &mut pool,
            2,
            Some(image(ContentType::Color, UVec2::splat(4), &color)),
        ));
        assert_eq!(c.content(), ContentType::Color);
        let nothing = Filed::Found(Found::Nothing);
        assert_eq!(pass.file(&mut pool, 3, None), nothing);
        let empty = image(ContentType::Mask, UVec2::ZERO, &[]);
        assert_eq!(pass.file(&mut pool, 4, Some(empty)), nothing);
        let past_slot = image(ContentType::Mask, UVec2::new(1 << 16, 1), &[]);
        assert_eq!(pass.file(&mut pool, 5, Some(past_slot)), nothing);
        let past_page = image(ContentType::Mask, UVec2::new(33, 1), &mask[..33]);
        assert_eq!(pass.file(&mut pool, 6, Some(past_page)), nothing);
        assert_eq!(pass.atlas.counters.oversized.count(), 1);
        for key in 3..=6 {
            assert_eq!(pass.atlas.find(&mut pool, &key), Some(Found::Nothing));
        }
        let queued: Vec<(AtlasFormat, U16Vec2)> = pass
            .uploads
            .copies
            .iter()
            .map(|copy| (copy.format, copy.placement.size))
            .collect();
        assert_eq!(
            queued,
            [
                (AtlasFormat::R8, U16Vec2::new(10, 12)),
                (AtlasFormat::Rgba8, U16Vec2::splat(4))
            ]
        );

        let full = vec![0; 32 * 32];
        for key in 10..14 {
            let filed = pass.file(
                &mut pool,
                key,
                Some(image(ContentType::Mask, UVec2::splat(32), &full)),
            );
            if key < 13 {
                raster(filed);
            } else {
                assert_eq!(filed, Filed::Full, "the hard cap, all drawn this frame");
            }
        }
        assert_eq!(pass.uploads.copies.len(), 2 + 3);
    }

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
