//! Turning a row of laid-out text into the glyph instances a pass draws.

//! Per-batch instance emission: extracted glyph placements →
//! `GlyphInstance`s.
//!
//! Two paths:
//!
//! - **Cache hit**: prior frames laid this exact `(TextShapeKey,
//!   scale, subpixel origin bin, area color)` run out into the atlas;
//!   the resulting origin-relative `GlyphInstance` templates are stored
//!   in the [`EncodedCache`]. Emit = a copy with origin-shifted
//!   positions, no shaper session, no per-glyph atlas hashmap lookup.
//!   This is the ~37% of frame time we're targeting.
//! - **Cache miss**: extracts the run's glyph placements through the
//!   shaper's render-session lease, touches/inserts atlas slots, emits
//!   to `out`, and populates the cache entry with the origin-relative
//!   templates so the next frame at the same `(key, scale, bins,
//!   color)` lands on the fast path. Runs that came out short — lines
//!   y-culled against their bounds, or a glyph the full atlas had no
//!   room for — are *not* cached: the key records neither bounds nor
//!   atlas occupancy, so a template with a hole would replay it on
//!   every hit and never retry.
//!
//! Atlas eviction reuses slot rectangles for new glyphs; any cached
//! entry holding the old uv would point at the wrong image. Each
//! encoded glyph therefore records its atlas slot's generation and
//! re-checks it while emitting. Atlas growth preserves rects
//! (`etagere::grow`), so no invalidation is needed there.

use crate::text::render::{
    GlyphImageKind, GlyphRasterKey, PlacedGlyph, RunPlacement, TextRenderSession,
};
use crate::text::request::TextShapeRequest;

use crate::renderer::backend::text::atlas::{GlyphAtlas, PackedGlyphMetadata};
use crate::renderer::backend::text::encode::EncodedRunKey;
use crate::renderer::backend::text::encode::cache::{
    ENCODED_CACHE_KEEP_FRAMES, EncodedCache, EncodedGlyph, release,
};
use crate::renderer::backend::text::{ContentType, GlyphInstance};

/// CPU-side glyph encoder: owns the atlas, the encoded-run cache, the
/// per-miss extraction scratch, and the frame's accumulated instances.
/// `TextBackend` owns one and partitions `instances` into per-batch
/// draw ranges; owning the state here lets every method borrow
/// disjoint fields directly, with no per-call context bundle.
#[derive(Debug)]
pub(crate) struct TextEncoder {
    pub(crate) atlas: GlyphAtlas,
    pub(crate) cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    pub(crate) placed: Vec<PlacedGlyph>,
    /// Drawable glyph instances accumulated across this frame's
    /// batches.
    pub(crate) instances: Vec<GlyphInstance>,
    /// Whether a run this frame hit a full atlas, and whether that has
    /// been reported since the last frame that didn't.
    ///
    /// Starvation is not corruption — the glyph is skipped, the run is
    /// refused as a template, and it re-encodes next frame — but it is
    /// silent, self-inflicted slowness with a visible hole in the text,
    /// and nothing else in the pipeline would say so. Edge-triggered
    /// because it recurs per glyph per run per frame; logging each one
    /// would bury the signal in its own noise.
    pub(crate) starved_this_frame: bool,
    pub(crate) starved_reported: bool,
}

impl TextEncoder {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self {
            atlas: GlyphAtlas::new(device),
            cache: EncodedCache::default(),
            placed: Vec::new(),
            instances: Vec::new(),
            starved_this_frame: false,
            starved_reported: false,
        }
    }

    /// Cache-hit fast path. Returns `true` if `run_key` resolved to a
    /// live entry and the run's glyphs were emitted; `false` falls
    /// through to [`Self::encode_run`].
    pub(crate) fn try_emit_cached(&mut self, run_key: &EncodedRunKey) -> bool {
        let current_frame = self.atlas.current_frame;
        let Some(entry) = self.cache.map.get_mut(&run_key.key) else {
            return false;
        };
        let glyphs = &self.cache.arena[entry.span.range()];
        let out_start = self.instances.len();
        self.instances.reserve(glyphs.len());
        let mut stale = false;
        // One pass emits the instance and refreshes the backing slot's
        // LRU stamp together, so `evict_one` can't reclaim a slot we're
        // still drawing this frame.
        for glyph in glyphs {
            let slot = &mut self.atlas.slots[glyph.atlas_slot as usize];
            if slot.generation != glyph.generation {
                self.instances.truncate(out_start);
                stale = true;
                break;
            }
            let g = glyph.instance;
            self.instances.push(GlyphInstance {
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
                let cache = &mut self.cache;
                release(&mut cache.arena, &mut cache.free_heads, dead.span);
            }
            return false;
        }
        entry.last_use = current_frame;
        self.cache.counters.hits.bump();
        true
    }

    /// Report the first starved run of an episode, so a full atlas is
    /// visible in a log rather than only as missing glyphs and a frame
    /// that quietly re-encodes everything.
    #[cold]
    fn note_atlas_starved(&mut self) {
        self.starved_this_frame = true;
        if self.starved_reported {
            return;
        }
        self.starved_reported = true;
        let bindings = self.atlas.bindings();
        tracing::warn!(
            mask_px = bindings.atlas_px[1],
            color_px = bindings.atlas_px[0],
            live_glyphs = self.atlas.cache.len(),
            "glyph atlas is full and cannot grow further; affected runs \
             drop glyphs and re-encode every frame until pressure clears",
        );
    }

    /// Frame teardown: take the shaper's `frame` clock into the atlas and
    /// sweep both caches against it.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        self.atlas.end_frame(frame);
        self.cache
            .sweep(self.atlas.current_frame, ENCODED_CACHE_KEEP_FRAMES);
        self.instances.clear();
        // A frame that fit everything closes the episode, so a later
        // recurrence is reported again rather than swallowed forever.
        if !self.starved_this_frame {
            self.starved_reported = false;
        }
        self.starved_this_frame = false;
    }

    /// Encode one run that missed the encoded cache: extract its glyph
    /// placements through the shaper `session` (which restores evicted
    /// buffers and applies the y-cull), touch/insert atlas slots, emit
    /// `GlyphInstance`s and populate the encoded cache as a side
    /// effect. Callers are expected to have already filtered out
    /// invalid keys and cache hits.
    pub(crate) fn encode_run(
        &mut self,
        device: &wgpu::Device,
        session: &mut TextRenderSession<'_>,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        run_key: EncodedRunKey,
    ) {
        let current_frame = self.atlas.current_frame;
        self.cache.counters.encodes.bump();
        // The straight-linear cast of the run's colour — already baked
        // into the cache identity, reused as the emit colour.
        let color = run_key.key.area_color;

        // `culled` records whether the extraction dropped any line — see
        // `EncodedCache::settle` for why that bars caching.
        let culled = session.extract_glyphs(request, placement, &mut self.placed);
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
            let idx = match self.atlas.touch(&g.raster_key) {
                Some(i) => i,
                None => {
                    match rasterize_and_insert(device, session, &mut self.atlas, g.raster_key) {
                        Rasterized::Slot(i) => i,
                        Rasterized::NoImage => continue,
                        Rasterized::AtlasFull => {
                            starved = true;
                            continue;
                        }
                    }
                }
            };
            let slot = self.atlas.slots[idx as usize];

            if slot.alloc.is_none() {
                continue;
            }

            let abs_x = g.x + slot.left as i32;
            let abs_y = g.y - slot.top as i32;
            let dim = (slot.width as u32) | ((slot.height as u32) << 16);
            let uv_and_kind = pack_uv(slot.x, slot.y, slot.content);

            self.instances.push(GlyphInstance {
                pos: [abs_x, abs_y],
                dim,
                uv_and_kind,
                color,
            });
            self.cache.pending.push(EncodedGlyph {
                instance: GlyphInstance {
                    pos: [abs_x - run_key.origin_x, abs_y - run_key.origin_y],
                    dim,
                    uv_and_kind,
                    color,
                },
                atlas_slot: idx,
                generation: slot.generation,
            });
        }

        if starved {
            self.note_atlas_starved();
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

/// Pack `(u, v, kind)` into the 32-bit `uv_and_kind` field. `u`'s
/// high bit carries `content_type` (atlases cap at 16384 = 14 bits).
#[inline]
pub(super) fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
    debug_assert!(u <= 0x7FFF, "uv high bit reserved for content_type");
    (u as u32) | ((kind as u32) << 15) | ((v as u32) << 16)
}

/// What [`rasterize_and_insert`] managed to do with one glyph. The two
/// failures are kept apart because only one of them is transient, and
/// [`EncodedCache::settle`] has to know which it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rasterized {
    /// Slab index of the glyph's atlas slot.
    Slot(u32),
    /// The font produced no image for this key. Permanent — the same
    /// key rasterizes to nothing next frame too, so a run that skips
    /// this glyph is still a complete encode.
    NoImage,
    /// The atlas is at the device maximum with no evictable rectangle.
    /// The glyph is missing *this frame only*, so the run must not be
    /// cached as a template.
    AtlasFull,
}

/// Cache miss path: ask the shaper session for the bitmap, push into
/// the atlas. A free fn, not a `TextEncoder` method: it's called while
/// `self.placed` is being iterated, so it may borrow only the disjoint
/// atlas field.
fn rasterize_and_insert(
    device: &wgpu::Device,
    session: &mut TextRenderSession<'_>,
    atlas: &mut GlyphAtlas,
    key: GlyphRasterKey,
) -> Rasterized {
    let Some(image) = session.rasterize(key) else {
        return Rasterized::NoImage;
    };
    let content = match image.kind {
        GlyphImageKind::Color => ContentType::Color,
        GlyphImageKind::Mask => ContentType::Mask,
    };
    let Ok(metadata): Result<PackedGlyphMetadata, _> = (&image.placement).try_into() else {
        tracing::warn!(
            ?key,
            width = image.placement.width,
            height = image.placement.height,
            left = image.placement.left,
            top = image.placement.top,
            "skipping glyph raster outside packed atlas metadata range",
        );
        return Rasterized::Slot(atlas.insert_unallocated(
            key,
            content,
            PackedGlyphMetadata::EMPTY,
        ));
    };

    if metadata.is_empty() {
        return Rasterized::Slot(atlas.insert_unallocated(key, content, metadata));
    }
    match atlas.insert(device, key, content, metadata, &image.data) {
        Some(idx) => Rasterized::Slot(idx),
        None => Rasterized::AtlasFull,
    }
}
