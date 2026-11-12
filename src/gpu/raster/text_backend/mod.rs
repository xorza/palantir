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
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::raster_pipeline::RasterPipelinePair;
use crate::gpu::raster::raster_pass::{RasterPass, RasterPassConfig};
use crate::gpu::raster::text_backend::encode::EncodedRunKey;
use crate::gpu::raster::text_backend::encode::encoder::{MissedRun, TextEncoder};
use crate::primitives::text::interned_text::InternedText;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::text::render::{GlyphRasterKey, RunPlacement};
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

    /// Append-mode prepare. Encoded-cache hits bypass shaping; the first miss
    /// opens the exclusive glyph lease.
    pub(crate) fn prepare_batch(&mut self, pool: &mut AtlasPool, batch: TextBatchInput<'_>) {
        let TextBatchInput {
            idx,
            runs,
            scale,
            interned_text,
        } = batch;
        self.encoder.sync_fonts(self.shaper.font_epoch());
        self.pass.open_batch(idx);

        // One walk: hits emit directly, misses encode through the lazily opened
        // lease, so an all-hit frame never touches cosmic.
        let mut glyphs = None;
        for r in runs {
            let run_key = EncodedRunKey::for_row(r, scale);
            let text = interned_text.resolve(r.text.span);
            let start = self.pass.instance_count();
            if !self
                .encoder
                .try_emit_cached(pool, &mut self.pass, &run_key, text)
            {
                let glyphs = glyphs.get_or_insert_with(|| self.shaper.glyphs());
                self.encoder.encode_run(
                    pool,
                    &mut self.pass,
                    glyphs,
                    MissedRun {
                        request: r.text.resolve_request(&interned_text),
                        placement: RunPlacement {
                            origin: run_key.origin(),
                            scale: scale * r.scale,
                            // A turned run's lines reach rows its unturned ones do not, so it
                            // culls none against its bounds.
                            bounds: r.spin.is_none().then_some(r.bounds),
                        },
                        key: run_key,
                    },
                );
            }
            // A turned run is emitted as an unturned one, then turned whole; its batch's scissor
            // clips it. An unturned one clips to its bounds here, so it batches with any run.
            match r.spin {
                Some(spin) => self.pass.turn_since(start, spin, r.clip),
                None => self.pass.clip_since(start, r.bounds, r.clip),
            }
        }
    }

    /// The shaper this backend encodes against, lent to a `GpuView` through
    /// [`GpuInitCtx`](crate::GpuInitCtx).
    pub(crate) const fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    /// Frame teardown for every submit, even one with no text batch; the last step
    /// of `WgpuBackend::submit`.
    ///
    /// Both caches age against the shaper's clock, so a text-free frame still
    /// sweeps ([`RasterPass::end_frame`]). Returns that clock so the icon atlas
    /// ages on the same span.
    pub(crate) fn end_frame(&mut self, pool: &mut AtlasPool) -> u64 {
        let frame = self.shaper.frame();
        self.pass.end_frame(pool, frame);
        self.encoder.end_frame(frame);
        frame
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

        /// One frame boundary as a window drives it: tick the shared text clock
        /// (owned by the record pass in production), then sweep this side. For
        /// harnesses with no `Ui`, since [`TextBackend::end_frame`] only reads the clock.
        pub(crate) fn tick_frame(&mut self, pool: &mut AtlasPool) {
            self.shaper.tick_frame();
            self.end_frame(pool);
        }
    }
}

#[cfg(test)]
mod tests;
