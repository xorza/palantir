//! Palantir-native glyph atlas + text render pipeline.
//!
//! Built to Palantir's contracts:
//!
//! - **Linear-premul end to end.** Straight-alpha linear f16 in,
//!   shader writes `vec4(rgb*a, a)`, blend is
//!   `PREMULTIPLIED_ALPHA_BLENDING`. No sRGB encode/decode round-trip.
//! - **Scissor does the clipping.** No per-glyph CPU clip; composer
//!   group scissor crops; cheap y-range pre-cull keeps off-screen
//!   lines out of the atlas cache.
//! - **One bind group, one atlas struct.** An `Rgba8UnormSrgb` colour
//!   texture and an `R8Unorm` mask texture side by side; the content
//!   type bits select in the shader.
//! - **GPU-blit on atlas grow.** `copy_texture_to_texture` from old
//!   to new; etagere preserves rects so the cache map stays intact —
//!   no re-rasterization.
//! - **Batched glyph uploads on cache miss.** Rasterized pixels queue
//!   into a retained staging buffer and flush as one belt write + N
//!   `copy_buffer_to_texture` commands on the main encoder, recorded
//!   *after* any grow blit — encoder ordering is load-bearing
//!   (`queue.write_texture` runs before all encoder commands in a
//!   submit, so it could be clobbered by the blit).
//! - **28-byte [`RasterQuad`](crate::gpu::raster::raster_atlas::raster_quad::RasterQuad)
//!   instances.** The content type and the desaturate flag sit above `u`
//!   in `uv_and_kind`.
//! - **No atlas sizes in the shader.** It reads each texel by index, so
//!   a grow changes the bind group alone — no uniform buffer, no
//!   per-batch push.

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
    /// Build the format-independent text resources (glyph atlas, shaper,
    /// caches, shader, vertex buffer). The render pipelines are built per
    /// format by [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines)
    /// from [`RasterProgram::build_variants`].
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
                        // Large enough to skip the 256->512->1024 grow
                        // chain on the first frame with non-trivial text.
                        initial_mask_px: 1024,
                        // Colour glyphs (emoji) are rare in UI text: 256^2 RGBA is
                        // 256 KB and holds dozens at UI sizes, where matching the
                        // mask side would pin 4 MB most sessions never touch.
                        initial_color_px: 256,
                        // 16 MiB is 2^24, and both `bytes_per_pixel` values are
                        // powers of two, so the ceiling lands on an exact power-of-
                        // two side either way: a 4096² mask or a 2048² colour
                        // atlas. The measured `text_atlas/cache_churn` working set
                        // is 3700 glyphs in a 2048² mask, so the mask ceiling is
                        // roughly 4x the largest set any bench here produces.
                        max_bytes: 16 << 20,
                        // 4 MiB is a 2048² mask or a 1024² colour atlas, and the
                        // mask growing 1 MB -> 4 MB is what the measurement in
                        // `eager_growth_bytes` cost.
                        eager_growth_bytes: 4 << 20,
                    },
                    initial_instances: 4096,
                },
            ),
        }
    }

    /// Append-mode prepare. Encoded-cache hits bypass shaping; the
    /// first miss opens the exclusive glyph lease, and each miss
    /// extracts and rasterizes its glyphs in place.
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

        // One walk: hits emit straight to `instances`; misses encode
        // through the lazily-opened lease. An all-hit frame never
        // cracks the RefCell or hits cosmic.
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

    /// The shaper this backend encodes against, for lending to a `GpuView`
    /// through [`GpuInitContext`](crate::GpuInitContext) — the one the whole window is
    /// already drawing text with.
    pub(crate) const fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    /// Frame teardown, run for every submit — including one that
    /// prepared no text batch at all.
    ///
    /// `end_frame`, not `post_record`: this runs as the last step of
    /// `WgpuBackend::submit`, nowhere near a record pass, and the crate
    /// spends `post_record` on the record half of a frame
    /// (`FrameCycle`, `Forest`, `Tree`). It belongs with the other
    /// frame-boundary teardowns instead — `TextSystem::end_frame` is its
    /// opposite number on the record side.
    ///
    /// Both caches age against the shaper's clock
    /// ([`TextShaper::frame`](crate::text::shaper::TextShaper::frame)),
    /// so a text-free frame still sweeps — see
    /// [`RasterPass::end_frame`].
    ///
    /// Returns that clock, for the icon atlas to age on as well: a keep
    /// count then means the same span in either tenant of a
    /// `RasterAtlas`.
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
        /// One frame boundary the way a window drives it: advance the
        /// shared text clock — owned by the record pass in production,
        /// where `TextSystem`'s frame teardown ticks it before the
        /// submit — then sweep this side against it.
        ///
        /// Harnesses that drive a `TextBackend` with no `Ui` behind it
        /// have no other way to age these caches, since
        /// [`TextBackend::end_frame`] only *reads* the clock.
        pub(crate) fn tick_frame(&mut self) {
            self.shaper.tick_frame();
            self.end_frame();
        }
    }
}

#[cfg(test)]
mod tests;
