//! Palantir-native glyph atlas + text render pipeline.
//!
//! Built to Palantir's contracts:
//!
//! - **Linear-premul end to end.** `ColorU8` is straight-linear-u8 in,
//!   shader writes `vec4(rgb*a, a)`, blend is
//!   `PREMULTIPLIED_ALPHA_BLENDING`. No sRGB encode/decode round-trip.
//! - **Scissor does the clipping.** No per-glyph CPU clip; composer
//!   group scissor crops; cheap y-range pre-cull keeps off-screen
//!   lines out of the atlas cache.
//! - **One bind group, one atlas struct.** Color + mask textures
//!   side by side; content_type bit selects in the shader.
//! - **GPU-blit on atlas grow.** `copy_texture_to_texture` from old
//!   to new; etagere preserves rects so the cache map stays intact —
//!   no re-rasterization.
//! - **Batched glyph uploads on cache miss.** Rasterized pixels queue
//!   into a retained staging buffer and flush as one belt write + N
//!   `copy_buffer_to_texture` commands on the main encoder, recorded
//!   *after* any grow blit — encoder ordering is load-bearing
//!   (`queue.write_texture` runs before all encoder commands in a
//!   submit, so it could be clobbered by the blit).
//! - **20-byte instances** (vs glyphon's 24). content_type packed
//!   into uv high bit.
//! - **No `Viewport` object.** Atlas sizes ride the shared immediate
//!   region as two `u32`s, pushed per batch — no uniform buffer.

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod encode;
mod encoded_counters;

use crate::primitives::interned_text::InternedText;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::raster_atlas::RasterAtlasConfig;
use crate::renderer::backend::raster_pass::{RasterPass, RasterPassConfig, RasterPassLabels};
use crate::renderer::backend::text::encode::EncodedRunKey;
use crate::renderer::backend::text::encode::encoder::TextEncoder;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::render::{GlyphRasterKey, RunPlacement};
use crate::text::shaper::TextShaper;

#[derive(Debug)]
pub(crate) struct TextBackend {
    shaper: TextShaper,
    encoder: TextEncoder,
    pub(super) pass: RasterPass<GlyphRasterKey>,
}

impl TextBackend {
    /// Build the format-independent text resources (glyph atlas, shaper,
    /// caches, shader, vertex buffer). The render pipelines are built per
    /// format by [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`RasterPass::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, shaper: TextShaper) -> Self {
        Self {
            shaper,
            encoder: TextEncoder::default(),
            pass: RasterPass::new(
                device,
                RasterPassConfig {
                    labels: RasterPassLabels {
                        shader: "palantir.text.shader",
                        vbuf: "palantir.text.vbuf",
                        pipeline: "palantir.text.pipeline",
                        stencil_pipeline: "palantir.text.pipeline.stencil_test",
                        layout: "palantir.text.pl",
                    },
                    atlas: RasterAtlasConfig {
                        label: "palantir.text",
                        // Bumped from glyphon's 256 to skip the 256->512->1024
                        // grow chain on the first frame with non-trivial text.
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
    /// extracts and rasterizes its glyphs in place. Rebinds the atlas
    /// bind group if it grew.
    pub(super) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        self.pass.open_batch(batch_idx);

        // One walk: hits emit straight to `instances`; misses encode
        // through the lazily-opened lease. An all-hit frame never
        // cracks the RefCell or hits cosmic.
        let mut glyphs = None;
        for r in runs {
            debug_assert!(
                !r.text.key.is_invalid(),
                "a run with no shaped buffer is dropped at the encoder and must \
                 not reach a batch",
            );
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
                    origin: r.origin,
                    scale: scale * r.scale,
                    bounds: Some(r.bounds),
                },
                run_key,
            );
        }
    }

    /// The shaper this backend encodes against, for lending to a `GpuView`
    /// through [`GpuInitCtx`](crate::GpuInitCtx) — the one the whole window is
    /// already drawing text with.
    pub(crate) fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    /// The shared cache clock these caches age against. The icon atlas
    /// ages on it too, so a keep count means the same span in either
    /// tenant of a `RasterAtlas`.
    pub(super) fn frame(&self) -> u64 {
        self.shaper.frame()
    }

    /// Frame teardown, run for every submit — including one that
    /// prepared no text batch at all.
    ///
    /// `end_frame`, not `post_record`: this runs as the last step of
    /// `WgpuBackend::submit`, nowhere near a record pass, and the crate
    /// spends `post_record` on the record half of a frame
    /// (`FrameCycle`, `Forest`, `Tree`). It belongs with the other
    /// frame-boundary teardowns instead — `TextSystem::end_frame`
    /// is its
    /// opposite number on the record side.
    ///
    /// Both caches age against the shaper's clock
    /// ([`TextShaper::frame`](crate::text::shaper::TextShaper::frame)),
    /// so a text-free frame still sweeps — see
    /// [`RasterPass::end_frame`].
    pub(crate) fn end_frame(&mut self) {
        let frame = self.shaper.frame();
        self.pass.end_frame(frame);
        self.encoder.end_frame(frame);
    }
}

// Both consumers need a real device, so both sit behind `internals`: the
// `text_atlas` benchmark (`bench` implies it) and the GPU regression suite
// in `tests.rs`. A plain `cargo test` build has neither, and neither does a
// non-test `internals` build.
#[cfg(all(feature = "internals", any(test, feature = "bench")))]
pub(crate) mod test_support {
    use crate::renderer::backend::text::TextBackend;

    impl TextBackend {
        /// One frame boundary the way a window drives it: advance the
        /// shared text clock — owned by the record pass in production,
        /// where `TextSystem`'s frame teardown ticks it before the
        /// submit —
        /// then sweep this side against it.
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
