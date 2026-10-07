//! Glyph atlas and text render pipeline.
//!
//! - Linear premultiplied end to end: the shader writes `vec4(rgb*a, a)`
//!   with `PREMULTIPLIED_ALPHA_BLENDING`; no sRGB round-trip.
//! - Scissor clips; no per-glyph CPU clip, only a y-range pre-cull.
//! - One bind group: a colour texture and a mask texture side by side.
//! - Atlas grow blits GPU-side; rects are preserved, so no re-rasterization.
//! - Glyph uploads are staged and flushed after any grow blit, because
//!   `queue.write_texture` runs before encoder commands and the blit could
//!   clobber it.
//! - The shader reads texels by index, so a grow changes only the bind group.

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod encode;
mod encoded_counters;

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
use crate::gpu::raster::raster_pass::{RasterPass, RasterPassConfig};
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::raster::text_backend::encode::EncodedRunKey;
use crate::gpu::raster::text_backend::encode::encoder::TextEncoder;
use crate::primitives::text::interned_text::InternedText;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::render::{GlyphRasterKey, RunPlacement};
use crate::text::shaper::TextShaper;

#[derive(Debug)]
pub(crate) struct TextBackend {
    shaper: TextShaper,
    encoder: TextEncoder,
    pass: RasterPass<GlyphRasterKey>,
}

impl TextBackend {
    /// Build the format-independent text resources; per-format pipelines come from
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines).
    pub(crate) fn new(device: &wgpu::Device, program: &RasterProgram, shaper: TextShaper) -> Self {
        Self {
            shaper,
            encoder: TextEncoder::default(),
            pass: RasterPass::new(
                device,
                program,
                RasterPassConfig {
                    vbuf: "palantir.text.vbuf",
                    atlas: RasterAtlasConfig {
                        label: "palantir.text",
                        // Skips the 256->512->1024 grow chain on the first text-heavy frame.
                        initial_mask_px: 1024,
                        // Colour glyphs are rare: 256^2 RGBA holds dozens, vs 4 MB at mask size.
                        initial_color_px: 256,
                        // 16 MiB lands on an exact power-of-two side: a 4096^2 mask or 2048^2
                        // colour atlas, ~4x the largest `text_atlas/cache_churn` set.
                        max_bytes: 16 << 20,
                        // 4 MiB is a 2048^2 mask or 1024^2 colour atlas; see `eager_growth_bytes`.
                        eager_growth_bytes: 4 << 20,
                    },
                    initial_instances: 4096,
                },
            ),
        }
    }

    /// Append-mode prepare. Encoded-cache hits bypass shaping; the first miss
    /// opens the exclusive glyph lease.
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        self.encoder.sync_fonts(self.shaper.font_epoch());
        self.pass.open_batch(batch_idx);

        // One walk: hits emit directly, misses encode through the lazily opened
        // lease, so an all-hit frame never touches cosmic.
        let mut glyphs = None;
        for r in runs {
            let run_key = EncodedRunKey::for_row(r, scale);
            if self.encoder.try_emit_cached(&mut self.pass, &run_key) {
                continue;
            }
            let glyphs = glyphs.get_or_insert_with(|| self.shaper.glyphs());
            self.encoder.encode_run(
                &mut self.pass,
                ctx.device,
                glyphs,
                r.text.resolve_request(interned_text),
                RunPlacement {
                    origin: run_key.origin(),
                    scale: scale * r.scale,
                    bounds: Some(r.bounds),
                },
                run_key,
            );
        }
    }

    /// The shaper this backend encodes against, lent to a `GpuView` through
    /// [`GpuInitContext`](crate::GpuInitContext).
    pub(crate) const fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    /// Frame teardown for every submit, even one with no text batch; the last step
    /// of `WgpuBackend::submit`.
    ///
    /// Both caches age against the shaper's clock, so a text-free frame still
    /// sweeps ([`RasterPass::end_frame`]). Returns that clock so the icon atlas
    /// ages on the same span.
    pub(crate) fn end_frame(&mut self) -> u64 {
        let frame = self.shaper.frame();
        self.pass.end_frame(frame);
        self.encoder.end_frame(frame);
        frame
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

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use crate::gpu::raster::text_backend::TextBackend;

    impl TextBackend {
        /// One frame boundary as a window drives it: tick the shared text clock
        /// (owned by the record pass in production), then sweep this side. For
        /// harnesses with no `Ui`, since [`TextBackend::end_frame`] only reads the clock.
        pub(crate) fn tick_frame(&mut self) {
            self.shaper.tick_frame();
            self.end_frame();
        }
    }
}

#[cfg(test)]
mod tests;
