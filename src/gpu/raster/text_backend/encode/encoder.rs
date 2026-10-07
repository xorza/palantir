//! The miss half of the hit/miss split in [the module doc](super): turns a
//! laid-out text row into glyph instances. The hit half is [`EncodedCache::emit_cached`].

use crate::text::glyphs::TextGlyphs;
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;

use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::raster::raster_pass::{RasterPass, Rasterized};
use crate::gpu::raster::text_backend::encode::EncodedRunKey;
use crate::gpu::raster::text_backend::encode::cache::{EncodedCache, EncodedGlyph};
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use glam::IVec2;

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
    /// Drops every encoded row when the shaper's font database has moved,
    /// before a batch emits, since stale templates would paint the wrong glyphs.
    pub(crate) fn sync_fonts(&mut self, font_epoch: u32) {
        if self.font_epoch != font_epoch {
            self.font_epoch = font_epoch;
            self.cache.clear();
        }
    }

    /// Cache-hit fast path: `true` if `run_key` hit and was emitted, else fall
    /// through to [`Self::encode_run`]. A forward because only the caller can
    /// open the glyph lease a miss needs, which an all-hit frame must not open.
    pub(crate) fn try_emit_cached(
        &mut self,
        pass: &mut RasterPass<GlyphRasterKey>,
        run_key: &EncodedRunKey,
    ) -> bool {
        self.cache.emit_cached(pass, run_key)
    }

    /// Sweeps the encoded-run cache against the shaper's `frame` clock.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        self.cache.sweep(frame);
    }

    /// Encodes one run that missed the cache: extracts placements through the
    /// shaper's lease, touches or inserts atlas slots, emits `RasterQuad`s and
    /// fills the cache. Callers filter invalid keys and hits first.
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
        let color: RgbaF16 = bytemuck::cast(run_key.key.area_color);

        // Whether extraction dropped any line; see `EncodedCache::settle`.
        let culled = glyphs.extract_glyphs(request, placement, placed);
        // The same for a glyph the atlas had no room for.
        let mut starved = false;

        // Slots used this frame are not eviction candidates, so a mid-walk eviction can't invalidate a template.
        for g in placed.iter() {
            let idx = if let Some(i) = pass.atlas.touch(&g.raster_key) {
                i
            } else {
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
            };
            let slot = pass.atlas.slots[idx as usize];
            let Some(placement) = slot.placement else {
                continue;
            };

            let quad = placement.quad(IVec2::new(g.x, g.y), color);
            pass.instances.push(quad);
            cache.stage(EncodedGlyph {
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

        // Partially visible or atlas-starved runs re-encode each frame; replaying a full template under narrower bounds is safe (the scissor clips).
        let complete = !culled && !starved;
        cache.settle(run_key.key, current_frame, complete);
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
