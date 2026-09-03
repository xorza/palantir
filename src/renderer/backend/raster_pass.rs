//! The GPU half both raster tenants own, in one type.
//!
//! An icon quad and a glyph quad are the same thing at the GPU level: a
//! tinted, atlas-sourced rectangle drawn at exactly the raster's pixel
//! dimensions. The two differ only in what fills the instance buffer —
//! cosmic and swash on one side, a baked SVG and resvg on the other —
//! and everything from the atlas a quad's uv points into to the `draw`
//! that consumes it is this file.
//!
//! What the tenants do **not** share is an instance of it. Each gets its
//! own atlas — its own textures, bind group, and eviction budget — so a
//! colour-icon-heavy frame cannot evict the glyphs of the label beside
//! it, and so the two can be sized for the content they actually hold.
//! The cost is one extra draw call on a group that mixes icons and text.

use crate::primitives::raster_image::RasterImage;
use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::raster_atlas::packed_metadata::PackedMetadata;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_atlas::{RasterAtlas, RasterAtlasConfig};
use crate::renderer::backend::raster_program::RasterProgram;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::backend::viewport::ViewportPush;
use std::fmt::Debug;
use std::hash::Hash;

/// Everything one tenant's pass settles at construction.
///
/// The pipeline and the shader are named by the [`RasterProgram`] that
/// owns them, since a frame capture wants one name per object. What is
/// per tenant is the instance buffer and the atlas.
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterPassConfig {
    /// GPU debug name for this tenant's instance buffer.
    pub(super) vbuf: &'static str,
    pub(super) atlas: RasterAtlasConfig,
    /// Quads the vertex buffer holds before its first growth. A screen of
    /// text runs to thousands of them and a screen of icons to hundreds,
    /// so the two tenants start far apart.
    pub(super) initial_instances: usize,
}

/// What one [`RasterPass::insert_raster`] managed to do with an image.
///
/// The two failures are kept apart because only one of them is
/// transient, and a caller that caches what it drew has to know which it
/// met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rasterized {
    /// Slab index of the image's atlas slot.
    Slot(u32),
    /// The atlas is at the device maximum with no evictable rectangle.
    /// The image is missing *this frame only*.
    AtlasFull,
}

/// The life of one atlas-starvation episode.
///
/// Starvation is not corruption — the image is skipped and re-encodes
/// next frame — but it is silent, self-inflicted slowness with a visible
/// hole in the frame, and nothing else in the pipeline would say so. It
/// is edge-triggered because it recurs per raster per frame, and logging
/// each one would bury the signal in its own noise.
///
/// Three named states rather than two bools: the bools admit a fourth
/// combination that means nothing, and a type that refuses it is one a
/// reader need not rule out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Starvation {
    /// No episode open: the last frame fit everything it drew.
    #[default]
    Clear,
    /// Reported, and this frame has starved too.
    Open,
    /// Reported, and this frame fit everything. One more such frame
    /// closes the episode, so a later recurrence is reported again
    /// rather than swallowed forever.
    Settling,
}

impl Starvation {
    /// Record a starved raster, answering whether it is the first of its
    /// episode and so the one worth a log line.
    fn note(&mut self) -> bool {
        let first = *self == Self::Clear;
        *self = Self::Open;
        first
    }

    /// Close a frame.
    fn end_frame(&mut self) {
        *self = match self {
            Self::Open => Self::Settling,
            Self::Clear | Self::Settling => Self::Clear,
        };
    }
}

#[derive(Debug)]
pub(super) struct RasterPass<K> {
    pub(super) atlas: RasterAtlas<K>,
    /// Drawable quads accumulated across this frame's batches.
    pub(super) instances: Vec<RasterQuad>,
    /// Where each batch's quads start in [`Self::instances`]. The next
    /// entry — or the instance count, for the last batch — is where they
    /// end, so a batch costs one push when it opens and needs no close.
    starts: Vec<u32>,
    vbuf: DynamicBuffer<RasterQuad>,
    /// Label stem, for the diagnostics that have to name a tenant.
    stem: &'static str,
    /// Where the atlas-starvation report stands — see [`Starvation`].
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
            stem: config.atlas.label,
            starvation: Starvation::default(),
        }
    }

    /// File a freshly rasterized image under `key` and return the slot it
    /// landed in.
    ///
    /// An image whose extents or bearing overflow [`PackedMetadata`], and
    /// one that covers no pixels at all, take a slot that owns no
    /// rectangle: the key is then a hit forever after, so the tenant pays
    /// the rasterizer once instead of on every frame that asks again.
    pub(super) fn insert_raster(
        &mut self,
        device: &wgpu::Device,
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
        match self
            .atlas
            .insert(device, key, image.content, metadata, image.data)
        {
            Some(idx) => Rasterized::Slot(idx),
            None => {
                self.note_atlas_starved();
                Rasterized::AtlasFull
            }
        }
    }

    /// Report the first starved raster of an episode, so a full atlas is
    /// visible in a log rather than only as a hole in the frame and a
    /// pass that quietly re-encodes everything.
    #[cold]
    fn note_atlas_starved(&mut self) {
        if !self.starvation.note() {
            return;
        }
        let atlas_px = self.atlas.atlas_px();
        tracing::warn!(
            label = self.stem,
            mask_px = atlas_px[1],
            color_px = atlas_px[0],
            live_rasters = self.atlas.cache.len(),
            "atlas is full and cannot grow further; affected batches drop \
             rasters and re-encode every frame until pressure clears",
        );
    }

    /// Note where batch `batch_idx`'s quads begin, before any is pushed.
    pub(super) fn open_batch(&mut self, batch_idx: usize) {
        debug_assert_eq!(
            batch_idx,
            self.starts.len(),
            "{} batches must be prepared once in contiguous order",
            self.stem,
        );
        self.starts.push(self.instances.len() as u32);
    }

    /// The instance range batch `batch_idx` draws. An empty span draws
    /// nothing.
    pub(super) fn batch_span(&self, batch_idx: usize) -> Span {
        let Some(&start) = self.starts.get(batch_idx) else {
            panic!(
                "render schedule referenced an unprepared {} batch",
                self.stem,
            );
        };
        let end = self
            .starts
            .get(batch_idx + 1)
            .copied()
            .unwrap_or(self.instances.len() as u32);
        Span::new(start, end - start)
    }

    /// Upload this frame's accumulated quads in one belt write, then drain
    /// the atlas's queued uploads (grow blits and per-raster texture
    /// copies) onto the renderer's encoder. Called once per frame, after
    /// every batch is prepared and before any pass draws — so the pixels
    /// land in the same submit as the draws that read them.
    ///
    /// One deferred write replaces N per-batch belt suballocations and
    /// copy commands over disjoint tails of the same `Vec`, and a
    /// mid-frame grow re-uploads at most once. Batch spans index the
    /// shared buffer, so per-batch draws are unaffected.
    pub(super) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.vbuf.upload_instances(ctx, &self.instances);
        self.atlas.flush_pending_uploads(ctx);
    }

    pub(super) fn render_batch<'a>(
        &'a self,
        batch_idx: usize,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        viewport: &ViewportPush,
    ) {
        let span = self.batch_span(batch_idx);
        self.atlas
            .draw_span(pass, pipelines, use_stencil, viewport, &self.vbuf, span);
    }

    /// Age the atlas against `frame` and drop this frame's quads and batch
    /// starts. Runs for every submit, including one that prepared no batch
    /// at all — a frame whose damage missed every raster still has to age
    /// the cache, or a keep count bounds only the frames that drew
    /// something rather than retention itself.
    pub(super) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            !self.starts.is_empty() || self.instances.is_empty(),
            "{} quads were emitted with no batch to draw them",
            self.stem,
        );
        self.atlas.advance_to(frame);
        self.instances.clear();
        self.starts.clear();
        self.starvation.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::backend::raster_pass::Starvation;

    /// The episode's whole life: reported once on the first starved
    /// raster, held open while starvation continues, and closed by the
    /// second clean frame so a later recurrence is reported again.
    ///
    /// The settling frame is the part worth pinning. Closing on the first
    /// clean frame instead would re-report a raster that starved again the
    /// very next frame, which is the per-frame noise the edge trigger
    /// exists to avoid.
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

        // Two clean frames from an open episode: the first settles, the
        // second closes.
        s.end_frame();
        assert_eq!(s, Starvation::Settling);
        s.end_frame();
        assert_eq!(s, Starvation::Clear);
        assert!(s.note(), "a fresh episode is reported again");
    }
}
