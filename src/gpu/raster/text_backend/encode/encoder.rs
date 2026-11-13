//! The run loop of a text batch: each run replays its cached template, the hit half
//! ([`EncodedCache::emit_cached`]), or is laid out and turned into glyph instances, the miss half.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::raster::raster_atlas::Found;
use crate::gpu::raster::raster_pass::{Filed, RasterPass};
use crate::gpu::raster::text_backend::TextBatchInput;
use crate::gpu::raster::text_backend::encode::EncodedRunKey;
use crate::gpu::raster::text_backend::encode::cache::{EncodeCoverage, EncodedCache, EncodedGlyph};
use crate::renderer::render_buffer::clip_chain::ClipChain;
use crate::renderer::render_buffer::text_draw_row::TextClip;
use crate::text::glyphs::TextGlyphs;
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;
use crate::text::shaper::TextShaper;

/// One run that missed the cache: what to shape, where it lands, and the key its template files under.
#[derive(Clone, Copy, Debug)]
struct MissedRun<'a> {
    request: TextShapeRequest<'a>,
    placement: RunPlacement,
    key: EncodedRunKey,
}

/// The glyph half of the text pass: the encoded-run cache and per-miss scratch.
#[derive(Debug, Default)]
pub(crate) struct TextEncoder {
    cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    placed: Vec<PlacedGlyph>,
    /// The shaper's font epoch at the last batch.
    font_epoch: u32,
}

impl TextEncoder {
    /// Emit `batch`'s runs into `pass`, in the batch it opened: a hit replays its template, a
    /// miss is laid out through the shaper's glyph lease, opened at the first miss, so an all-hit
    /// frame never touches cosmic. Every encoded row's age is the shaper's clock as it reads now,
    /// ticked by the frame's record pass, the clock the sweep at the submit's end reads. Drops
    /// every encoded row first where the shaper's font database moved, as stale templates would
    /// paint the wrong glyphs.
    pub(crate) fn encode_batch(
        &mut self,
        pool: &mut AtlasPool,
        pass: &mut RasterPass<GlyphRasterKey>,
        shaper: &TextShaper,
        batch: TextBatchInput<'_>,
    ) {
        let TextBatchInput {
            idx: _,
            runs,
            scale,
            interned_text,
        } = batch;
        let font_epoch = shaper.font_epoch();
        if self.font_epoch != font_epoch {
            self.font_epoch = font_epoch;
            self.cache.clear();
        }
        let frame = shaper.frame();
        let mut glyphs = None;
        for r in runs {
            let run_key = EncodedRunKey::for_row(r, scale);
            let text = interned_text.resolve(r.text.span);
            let start = pass.instance_count();
            if !self.cache.emit_cached(pool, pass, &run_key, text, frame) {
                let glyphs = glyphs.get_or_insert_with(|| shaper.glyphs());
                self.encode_run(
                    pool,
                    pass,
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
                    frame,
                );
            }
            // A turned run is emitted as an unturned one, then turned whole; its batch's scissor
            // clips it. An unturned one an edge cuts, or a rounded clip reaches, clips to its
            // bounds and takes the chain here, so it batches with any run; one neither touches
            // lies inside them already, as quads the encoded template holds with no chain.
            match r.spin {
                Some(spin) => pass.turn_since(start, spin, r.clip),
                None if r.reach == TextClip::Whole && r.clip == ClipChain::NONE => {
                    debug_assert!(
                        pass.holds_since(start, r.bounds),
                        "an uncut run's glyphs lie inside its bounds"
                    );
                }
                None => pass.clip_since(start, r.bounds, r.clip),
            }
        }
    }

    /// Sweeps the encoded-run cache against the shaper's `frame` clock.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        self.cache.sweep(frame);
    }

    /// Encodes one run that missed the cache at the shaper's `frame`: extracts placements through
    /// the shaper's lease, finds or files each glyph's raster, emits `RasterQuad`s and fills the
    /// cache.
    fn encode_run(
        &mut self,
        pool: &mut AtlasPool,
        pass: &mut RasterPass<GlyphRasterKey>,
        glyphs: &mut TextGlyphs<'_>,
        run: MissedRun<'_>,
        frame: u64,
    ) {
        let MissedRun {
            request,
            placement,
            key: run_key,
        } = run;
        let Self { cache, placed, .. } = self;
        cache.start_row();
        let color = run_key.color();

        // Whether extraction dropped any line; see `EncodedCache::settle`.
        let culled = glyphs.extract_glyphs(request, placement, placed);
        // The same for a glyph the atlas had no room for.
        let mut starved = false;

        // Rasters drawn this frame are not eviction candidates, so a mid-walk eviction cannot invalidate a template.
        for g in placed.iter() {
            let found = match pass.atlas.find(pool, &g.raster_key) {
                Some(found) => found,
                None => match pass.file(pool, g.raster_key, glyphs.rasterize(g.raster_key)) {
                    Filed::Found(found) => found,
                    Filed::Full => {
                        starved = true;
                        continue;
                    }
                },
            };
            let Found::Raster(placement) = found else {
                continue;
            };
            let quad = placement.quad(g.physical, color);
            pass.push(quad);
            cache.stage(EncodedGlyph {
                offset: quad.pos - run_key.pixel,
                handle: placement.handle,
            });
        }

        // Partially visible or atlas-starved runs re-encode each frame; replaying a full template
        // under narrower bounds is safe, as the run clips to them (`RasterPass::clip_since`).
        let coverage = if culled || starved {
            EncodeCoverage::Partial
        } else {
            EncodeCoverage::Complete
        };
        cache.settle(run_key.key, request.text(), frame, coverage);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::raster::text_backend::encode::cache::EncodedCache;
    use crate::gpu::raster::text_backend::encode::encoder::TextEncoder;

    impl TextEncoder {
        pub(crate) fn cache(&self) -> &EncodedCache {
            &self.cache
        }
    }
}
