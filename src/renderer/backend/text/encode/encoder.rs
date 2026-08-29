//! Turning a row of laid-out text into the glyph instances a pass draws.
//!
//! Both halves of the hit/miss split [the module doc](super) states live
//! here, and the atlas traffic each owes is what separates them. A hit
//! copies the cached templates with an origin shift, but must first
//! re-check every glyph's recorded slot generation: eviction hands a slot
//! rectangle to another glyph, and a template holding the old uv would
//! draw that glyph instead. A miss takes the shaper's glyph lease,
//! touches or rasterizes each glyph, and hands the finished row to
//! [`EncodedCache::settle`].
//!
//! Growth needs no such check — `etagere::grow` preserves rectangles, so
//! a cached uv survives it.
//!
//! Filing a raster in the atlas is not here: that is
//! [`RasterPass::insert_raster`], which the icon side calls with an SVG
//! where this one calls with a glyph.

use crate::text::glyphs::TextGlyphs;
use crate::text::render::{GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;

use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_pass::{RasterImage, RasterPass, Rasterized};
use crate::renderer::backend::text::encode::EncodedRunKey;
use crate::renderer::backend::text::encode::cache::{EncodedCache, EncodedGlyph};

/// The glyph-shaped half of the text pass: the encoded-run cache and the
/// per-miss extraction scratch. The atlas it fills and the instance
/// buffer it emits into belong to the [`RasterPass`] every method takes,
/// which the icon side fills the same way from an entirely different
/// rasterizer.
#[derive(Debug, Default)]
pub(crate) struct TextEncoder {
    pub(crate) cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    pub(crate) placed: Vec<PlacedGlyph>,
}

impl TextEncoder {
    /// Cache-hit fast path. Returns `true` if `run_key` resolved to a
    /// live entry and the run's glyphs were emitted; `false` falls
    /// through to [`Self::encode_run`].
    pub(crate) fn try_emit_cached(
        &mut self,
        pass: &mut RasterPass<GlyphRasterKey>,
        run_key: &EncodedRunKey,
    ) -> bool {
        let current_frame = pass.atlas.current_frame;
        let Some(entry) = self.cache.map.get_mut(&run_key.key) else {
            return false;
        };
        let glyphs = &self.cache.arena.slots[entry.span.range()];
        let out_start = pass.instances.len();
        pass.instances.reserve(glyphs.len());
        let mut stale = false;
        // One pass emits the instance and refreshes the backing slot's
        // LRU stamp together, so `evict_one` can't reclaim a slot we're
        // still drawing this frame.
        for glyph in glyphs {
            let slot = &mut pass.atlas.slots[glyph.atlas_slot as usize];
            if slot.generation != glyph.generation {
                pass.instances.truncate(out_start);
                stale = true;
                break;
            }
            let g = glyph.instance;
            pass.instances.push(RasterQuad {
                pos: [g.pos[0] + run_key.origin_x, g.pos[1] + run_key.origin_y],
                dim: g.dim,
                uv_and_kind: g.uv_and_kind,
                color: g.color,
            });
            slot.last_use = current_frame;
        }
        if stale {
            // An eviction reused one of this run's slots, so the whole
            // template is dead. Drop the row now (the map borrow ends
            // here) rather than re-probing and re-walking it every
            // frame until the next sweep: `encode_run` only replaces it
            // if this run also survives the y-cull, so a culled run
            // would otherwise pay the failed lookup indefinitely.
            if let Some(dead) = self.cache.map.remove(&run_key.key) {
                self.cache.arena.release(dead.span);
            }
            return false;
        }
        entry.last_use = current_frame;
        self.cache.counters.hits.bump();
        true
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
        self.cache.counters.encodes.bump();
        // The straight-linear cast of the run's colour — already baked
        // into the cache identity, reused as the emit colour.
        let color = run_key.key.area_color;

        // `culled` records whether the extraction dropped any line — see
        // `EncodedCache::settle` for why that bars caching.
        let culled = glyphs.extract_glyphs(request, placement, &mut self.placed);
        // …and `starved` the same for a glyph the atlas had no room for.
        let mut starved = false;

        // Build a fresh cache entry as a side effect of the slow walk.
        // Slots used earlier this frame cannot be eviction candidates,
        // so an atlas eviction during the walk cannot invalidate a
        // template already appended here.
        debug_assert!(
            self.cache.pending.is_empty(),
            "settle clears the pending row, so every encode starts empty",
        );

        for g in self.placed.iter() {
            let idx = match pass.atlas.touch(&g.raster_key) {
                Some(i) => i,
                None => {
                    // No image at all is permanent — the same key
                    // rasterizes to nothing next frame too — so a run
                    // that skips this glyph is still a complete encode.
                    let Some(image) = glyphs.rasterize(g.raster_key) else {
                        continue;
                    };
                    let raster = RasterImage {
                        content: image.kind,
                        width: image.placement.width,
                        height: image.placement.height,
                        left: image.placement.left,
                        top: image.placement.top,
                        data: &image.data,
                    };
                    match pass.insert_raster(device, g.raster_key, raster) {
                        Rasterized::Slot(i) => i,
                        Rasterized::AtlasFull => {
                            starved = true;
                            continue;
                        }
                    }
                }
            };
            let slot = pass.atlas.slots[idx as usize];

            if slot.alloc.is_none() {
                continue;
            }

            let abs_x = g.x + slot.left as i32;
            let abs_y = g.y - slot.top as i32;
            let dim = RasterQuad::dim(slot.width, slot.height);
            let uv_and_kind = RasterQuad::pack_uv(slot.x, slot.y, slot.content);

            pass.instances.push(RasterQuad {
                pos: [abs_x, abs_y],
                dim,
                uv_and_kind,
                color,
            });
            self.cache.pending.push(EncodedGlyph {
                instance: RasterQuad {
                    pos: [abs_x - run_key.origin_x, abs_y - run_key.origin_y],
                    dim,
                    uv_and_kind,
                    color,
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
        self.cache.settle(run_key.key, current_frame, complete);
    }
}
