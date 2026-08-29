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
use crate::primitives::span::Span;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::stencil_variant::ColorVariantSpec;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::backend::viewport::ViewportPush;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::render::RunPlacement;
use crate::text::shaper::TextShaper;

use crate::renderer::backend::text::encode::encoder::TextEncoder;
#[derive(Debug)]
pub(crate) struct TextBackend {
    shaper: TextShaper,
    encoder: TextEncoder,

    /// Text shader module — format-independent; [`Self::build_variants`]
    /// reads it to build each format's pipelines.
    shader: wgpu::ShaderModule,

    vbuf: DynamicBuffer<RasterQuad>,

    /// Per-batch slice of the encoder's `instances`; empty span =
    /// nothing to draw.
    ranges: Vec<Span>,
}

impl TextBackend {
    /// Build the format-independent text resources (glyph atlas, shaper,
    /// caches, shader, vertex buffer). The render pipelines are built per
    /// format by [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// from [`Self::build_variants`].
    pub(crate) fn new(device: &wgpu::Device, shaper: TextShaper) -> Self {
        let encoder = TextEncoder::new(device);

        let shader = RasterQuad::shader_module(device, "palantir.text.shader");
        let vbuf = DynamicBuffer::<RasterQuad>::vertex(device, "palantir text vbuf", 4096);

        Self {
            shaper,
            encoder,
            shader,
            vbuf,
            ranges: Vec::new(),
        }
    }

    /// Build the base + stencil-test render pipelines against `format`,
    /// reading the format-independent `shader`. The glyph atlas, its bind
    /// group, and the sampler are not built here and so survive a format
    /// change. Called by `FormatPipelines` per format; matches the
    /// `build_variants` shape of the quad / mesh / image / curve pipelines.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Group 0 = atlas textures + sampler. Viewport + atlas sizes
        // ride the shared immediate region.
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.text.pipeline",
                stencil_label: "palantir.text.pipeline.stencil_test",
                layout_label: "palantir.text.pl",
                shader: &self.shader,
                bind_group_layouts: &[Some(self.encoder.atlas.bind_group_layout())],
                vertex_buffers: &[Some(RasterQuad::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Append-mode prepare. Encoded-cache hits bypass shaping; the
    /// first miss opens the exclusive glyph lease, and each miss
    /// extracts and rasterizes its glyphs in place. Rebinds the atlas
    /// bind group if it grew.
    pub(crate) fn prepare_batch(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        scale: f32,
        batch_idx: usize,
        runs: &[TextDrawRow],
        interned_text: &InternedText<'_>,
    ) {
        debug_assert_eq!(
            batch_idx,
            self.ranges.len(),
            "text batches must be prepared once in contiguous order",
        );
        let start = self.encoder.instances.len() as u32;

        // One walk: hits emit straight to `instances`; misses encode
        // through the lazily-opened lease. An all-hit frame never
        // cracks the RefCell or hits cosmic.
        let mut glyphs = None;
        for r in runs {
            if r.text.key.is_invalid() {
                // Backstop: the encoder already drops runs with no shaped
                // buffer, so nothing production-emitted reaches this.
                continue;
            }
            let run_key = encode::encode_key_for(r, scale);
            if self.encoder.try_emit_cached(&run_key) {
                continue;
            }
            let glyphs = glyphs.get_or_insert_with(|| self.shaper.glyphs());
            self.encoder.encode_run(
                ctx.device,
                glyphs,
                r.text.resolve_request(interned_text),
                RunPlacement {
                    origin: r.origin,
                    scale: scale * r.scale,
                    bounds: r.bounds,
                },
                run_key,
            );
        }
        drop(glyphs);

        let end = self.encoder.instances.len() as u32;

        self.ranges.push(Span::new(start, end - start));
    }

    /// Upload this frame's accumulated glyph instances in one belt
    /// write, then drain queued glyph-atlas uploads (grow blits +
    /// per-glyph texture copies) onto the renderer's encoder. Called
    /// once per frame, after every `prepare_batch` and before any pass
    /// draws — so atlas uploads share the same submit as the text
    /// draws that read from them. Deferring instances to a single
    /// write replaces N per-batch belt suballocations + copy commands
    /// for disjoint tails of the same Vec, and a mid-frame grow's full
    /// re-upload happens at most once; batch `ranges` index into the
    /// shared buffer, so per-batch draws are unaffected.
    /// The shaper this backend encodes against, for lending to a `GpuView`
    /// through [`GpuInitCtx`](crate::GpuInitCtx) — the one the whole window is
    /// already drawing text with.
    pub(crate) fn shaper(&self) -> &TextShaper {
        &self.shaper
    }

    pub(crate) fn flush(&mut self, ctx: &mut GpuCtx<'_>) {
        self.vbuf.upload_instances(ctx, &self.encoder.instances);
        self.encoder.atlas.flush_pending_uploads(ctx);
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
            .expect("render schedule referenced an unprepared text batch");
        self.encoder
            .atlas
            .draw_span(pass, pipelines, use_stencil, viewport, &self.vbuf, span);
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
    /// **Runs on an empty `ranges` too.** Returning early there would
    /// freeze this side's clock on any frame whose damage happened to
    /// miss every text run, while the shaper's kept advancing. Both
    /// caches age against the shaper's clock
    /// ([`TextShaper::frame`](crate::text::shaper::TextShaper::frame)),
    /// so sweeping a text-free frame is what keeps
    /// `ENCODED_CACHE_KEEP_FRAMES` a bound on retention rather than a
    /// bound on text-bearing frames.
    pub(crate) fn end_frame(&mut self) {
        debug_assert!(
            !self.ranges.is_empty() || self.encoder.instances.is_empty(),
            "instances were emitted without a batch range to draw them",
        );
        self.encoder.advance_to(self.shaper.frame());
        self.ranges.clear();
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
