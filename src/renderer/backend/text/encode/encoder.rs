//! Turning a row of laid-out text into the glyph instances a pass draws.
//!
//! The miss half of the hit/miss split [the module doc](super) states:
//! take the shaper's glyph lease, touch or rasterize each glyph, and
//! stage the finished row on the [`EncodedCache`]. The hit half is the
//! cache's own [`EncodedCache::emit_cached`], because what it must
//! re-check before it emits is the cache's recorded slot generations.
//!
//! Filing a raster in the atlas is not here either: that is
//! [`RasterPass::insert_raster`], which the icon side calls with an SVG
//! where this one calls with a glyph.

use crate::text::glyphs::TextGlyphs;
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;

use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_pass::{RasterPass, Rasterized};
use crate::renderer::backend::text::encode::EncodedRunKey;
use crate::renderer::backend::text::encode::cache::{EncodedCache, EncodedGlyph};
use glam::IVec2;

/// The glyph-shaped half of the text pass: the encoded-run cache and the
/// per-miss extraction scratch. The atlas it fills and the instance
/// buffer it emits into belong to the [`RasterPass`] every method takes,
/// which the icon side fills the same way from an entirely different
/// rasterizer.
#[derive(Debug, Default)]
pub(crate) struct TextEncoder {
    cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    placed: Vec<PlacedGlyph>,
    /// The shaper's font epoch as of the last batch — see
    /// [`Self::sync_fonts`].
    font_epoch: u32,
}

impl TextEncoder {
    /// Drop every encoded row when the shaper's font database has moved
    /// under it.
    ///
    /// Checked before a batch emits rather than swept after one: a
    /// registered font changes which physical face a family resolves to,
    /// and a frame that painted from stale templates has already painted
    /// the wrong glyphs. One `u32` compare per batch buys that.
    pub(crate) fn sync_fonts(&mut self, font_epoch: u32) {
        if self.font_epoch != font_epoch {
            self.font_epoch = font_epoch;
            self.cache.clear();
        }
    }

    /// Cache-hit fast path. Returns `true` if `run_key` resolved to a
    /// live entry and the run's glyphs were emitted; `false` falls
    /// through to [`Self::encode_run`].
    ///
    /// **A forward that stays one.** The hit/miss split has to live at
    /// the caller, because only the caller can open the glyph lease a
    /// miss needs and an all-hit frame must not open. Folding the two
    /// would put the shaper inside this type, and publishing the cache
    /// would widen it for one method.
    pub(crate) fn try_emit_cached(
        &mut self,
        pass: &mut RasterPass<GlyphRasterKey>,
        run_key: &EncodedRunKey,
    ) -> bool {
        self.cache.emit_cached(pass, run_key)
    }

    /// Sweep the encoded-run cache against the shaper's `frame` clock
    /// reading. The atlas beside it ages on the same reading through
    /// [`RasterPass::end_frame`].
    pub(crate) fn end_frame(&mut self, frame: u64) {
        self.cache.sweep(frame);
    }

    /// Encode one run that missed the encoded cache: extract its glyph
    /// placements through the shaper's glyph lease (which restores evicted
    /// buffers and applies the y-cull), touch/insert atlas slots, emit
    /// `RasterQuad`s and populate the encoded cache as a side
    /// effect. Callers are expected to have already filtered out
    /// invalid keys and cache hits.
    pub(crate) fn encode_run(
        &mut self,
        pass: &mut RasterPass<GlyphRasterKey>,
        device: &wgpu::Device,
        glyphs: &mut TextGlyphs<'_>,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        run_key: EncodedRunKey,
    ) {
        let current_frame = pass.atlas.current_frame;
        let Self {
            cache,
            placed,
            font_epoch: _,
        } = self;
        cache.start_row();
        // The straight-linear cast of the run's colour — already baked
        // into the cache identity, reused as the emit colour.
        let color = run_key.key.area_color;

        // `culled` records whether the extraction dropped any line — see
        // `EncodedCache::settle` for why that bars caching.
        let culled = glyphs.extract_glyphs(request, placement, placed);
        // …and `starved` the same for a glyph the atlas had no room for.
        let mut starved = false;

        // Build a fresh cache entry as a side effect of the slow walk.
        // Slots used earlier this frame cannot be eviction candidates,
        // so an atlas eviction during the walk cannot invalidate a
        // template already appended here.
        for g in placed.iter() {
            let idx = match pass.atlas.touch(&g.raster_key) {
                Some(i) => i,
                None => {
                    // No image at all is permanent — the same key
                    // rasterizes to nothing next frame too — so a run
                    // that skips this glyph is still a complete encode.
                    let Some(image) = glyphs.rasterize(g.raster_key) else {
                        continue;
                    };
                    match pass.insert_raster(device, g.raster_key, image) {
                        Rasterized::Slot(i) => i,
                        Rasterized::AtlasFull => {
                            starved = true;
                            continue;
                        }
                    }
                }
            };
            let slot = pass.atlas.slots[idx as usize];
            let Some(placement) = slot.placement else {
                continue;
            };

            let quad = placement.quad(IVec2::new(g.x, g.y), color);
            pass.instances.push(quad);
            cache.stage(EncodedGlyph {
                // The template is the same quad shifted to the run's
                // own origin, so a hit replays it by adding that back.
                instance: RasterQuad {
                    pos: [
                        quad.pos[0] - run_key.origin_x,
                        quad.pos[1] - run_key.origin_y,
                    ],
                    ..quad
                },
                atlas_slot: idx,
                generation: slot.generation,
            });
        }

        // The caller already filtered invalid keys; valid-key here is a
        // precondition. Partially visible or atlas-starved runs
        // re-encode each frame; the reverse (a cached full template
        // replayed under narrower bounds) is safe — the batch scissor is
        // the real clip.
        let complete = !culled && !starved;
        cache.settle(run_key.key, current_frame, complete);
    }
}

/// Reach-in for the GPU text tests, which assert on what a hit and an
/// invalidation leave in the encoded cache. Carries their `internals`
/// gate, because every reader `EncodedCache` offers them carries it too.
#[cfg(all(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::renderer::backend::text::encode::cache::EncodedCache;
    use crate::renderer::backend::text::encode::encoder::TextEncoder;

    impl TextEncoder {
        pub(crate) fn cache(&self) -> &EncodedCache {
            &self.cache
        }
    }
}
