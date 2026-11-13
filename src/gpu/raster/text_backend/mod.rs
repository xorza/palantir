//! Glyph atlas and text render pipeline.
//!
//! - Linear premultiplied end to end: the shader writes `vec4(rgb*a, a)`
//!   with `PREMULTIPLIED_ALPHA_BLENDING`; no sRGB round-trip.
//! - An unturned run's quads clip to its bounds on the CPU ([`RasterPass::clip_since`]) after a
//!   y-range pre-cull; a turned run takes its batch's scissor.
//! - Glyphs live in the atlas pool beside the icons (`gpu::atlas`), drawn through the one atlas
//!   group over its mask and color pages; a grow keeps every rectangle, so no re-rasterization.
//! - Glyph uploads are staged and flushed after the pool's commit, because
//!   `queue.write_texture` runs before encoder commands and a grow's copy could
//!   clobber it.
//! - The shader reads texels by index, so a grow changes only the bind group.

#[cfg(feature = "internals")]
pub(crate) mod bench;
mod encode;
mod encoded_counters;

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::RasterPipelinePair;
use crate::gpu::raster::raster_pass::{RasterPass, RasterPassConfig};
use crate::gpu::raster::text_backend::encode::encoder::TextEncoder;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::primitives::text::interned_text::InternedText;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::text::render::GlyphRasterKey;
use crate::text::shaper::TextShaper;

/// One text batch to prepare: its rows, the frame's raster scale, and the text they resolve against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextBatchInput<'a> {
    /// The batch's index in the frame; batches arrive in order.
    pub(crate) idx: usize,
    pub(crate) runs: &'a [TextDrawRow],
    pub(crate) scale: f32,
    pub(crate) interned_text: InternedText<'a>,
}

#[derive(Debug)]
pub(crate) struct TextBackend {
    shaper: TextShaper,
    encoder: TextEncoder,
    pass: RasterPass<GlyphRasterKey>,
}

impl TextBackend {
    /// Build the format-independent text resources; per-format pipelines come from
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(gpu: &Gpu, shaper: TextShaper) -> Self {
        Self {
            shaper,
            encoder: TextEncoder::default(),
            pass: RasterPass::new(
                gpu,
                RasterPassConfig {
                    label: "palantir.text",
                    tenant: Tenant::Glyphs,
                    vbuf: "palantir.text.vbuf",
                    spun_vbuf: "palantir.text.spun_vbuf",
                    initial_instances: 4096,
                },
            ),
        }
    }

    /// Append-mode prepare: the batch's runs into the batch it opens ([`TextEncoder::encode_batch`]).
    pub(crate) fn prepare_batch(&mut self, pool: &mut AtlasPool, batch: TextBatchInput<'_>) {
        self.pass.open_batch(batch.idx);
        self.encoder
            .encode_batch(pool, &mut self.pass, &self.shaper, batch);
    }

    /// The shaper this backend encodes against, lent to a `GpuView` through
    /// [`GpuInitCtx`](crate::GpuInitCtx).
    pub(crate) const fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    /// Drop the keys of the glyphs the pool evicted since the last drain: at the end of each submit,
    /// and after a purge.
    pub(crate) fn drop_evicted(&mut self, pool: &mut AtlasPool) {
        self.pass.atlas.drop_evicted(pool);
    }

    /// Frame teardown for every submit, even one with no text batch, after its drain: the encoded
    /// runs age on the shaper's clock, which their shaped buffers age on, and the glyph entries that
    /// draw nothing on `pool`'s display frames, which the atlas ages on.
    pub(crate) fn end_frame(&mut self, pool: &AtlasPool) {
        self.pass.end_frame(pool.frame());
        self.encoder.end_frame(self.shaper.frame());
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    use crate::gpu::atlas::AtlasPool;
    #[cfg(test)]
    use crate::gpu::raster::raster_atlas::counters::AtlasCounts;
    use crate::gpu::raster::text_backend::TextBackend;

    impl TextBackend {
        /// The glyph atlas's eviction and growth counts so far.
        #[cfg(test)]
        pub(crate) fn atlas_counts(&self) -> AtlasCounts {
            self.pass.atlas.counters.counts()
        }

        /// One frame boundary as a window's submit and its next record pass drive it, for
        /// harnesses with no `Ui`: the drain and this side's sweep at the clock the frame prepared
        /// at, then the shared text clock ticks for the next frame, as the record pass ticks it in
        /// production.
        pub(crate) fn tick_frame(&mut self, pool: &mut AtlasPool) {
            self.drop_evicted(pool);
            self.end_frame(pool);
            self.shaper.tick_frame();
        }
    }
}

#[cfg(test)]
mod tests;
