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

use crate::text::glyphs::TextGlyphs;
use crate::text::render::{GlyphImageKind, GlyphRasterKey, PlacedGlyph, RunPlacement};
use crate::text::request::TextShapeRequest;

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::packed_metadata::PackedMetadata;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_atlas::{RasterAtlas, RasterAtlasConfig};
use crate::renderer::backend::text::encode::EncodedRunKey;
use crate::renderer::backend::text::encode::cache::{EncodedCache, EncodedGlyph};

/// CPU-side glyph encoder: owns the atlas, the encoded-run cache, the
/// per-miss extraction scratch, and the frame's accumulated instances.
/// `TextBackend` owns one and partitions `instances` into per-batch
/// draw ranges; owning the state here lets every method borrow
/// disjoint fields directly, with no per-call context bundle.
#[derive(Debug)]
pub(crate) struct TextEncoder {
    pub(crate) atlas: RasterAtlas<GlyphRasterKey>,
    pub(crate) cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    pub(crate) placed: Vec<PlacedGlyph>,
    /// Drawable glyph instances accumulated across this frame's
    /// batches.
    pub(crate) instances: Vec<RasterQuad>,
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
            atlas: RasterAtlas::new(
                device,
                RasterAtlasConfig {
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
            ),
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
        let glyphs = &self.cache.arena.slots[entry.span.range()];
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
            self.instances.push(RasterQuad {
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
        let atlas_px = self.atlas.atlas_px();
        tracing::warn!(
            mask_px = atlas_px[1],
            color_px = atlas_px[0],
            live_glyphs = self.atlas.cache.len(),
            "glyph atlas is full and cannot grow further; affected runs \
             drop glyphs and re-encode every frame until pressure clears",
        );
    }

    /// Advance to the shaper's `frame` clock reading and sweep both
    /// caches against it. Named for what it does, not for the frame
    /// boundary its caller happens to sit on — see
    /// [`RasterAtlas::advance_to`](crate::renderer::backend::raster_atlas::RasterAtlas::advance_to).
    pub(crate) fn advance_to(&mut self, frame: u64) {
        self.atlas.advance_to(frame);
        self.cache.sweep(self.atlas.current_frame);
        self.instances.clear();
        // A frame that fit everything closes the episode, so a later
        // recurrence is reported again rather than swallowed forever.
        if !self.starved_this_frame {
            self.starved_reported = false;
        }
        self.starved_this_frame = false;
    }

    /// Encode one run that missed the encoded cache: extract its glyph
    /// placements through the shaper's glyph lease (which restores evicted
    /// buffers and applies the y-cull), touch/insert atlas slots, emit
    /// `RasterQuad`s and populate the encoded cache as a side
    /// effect. Callers are expected to have already filtered out
    /// invalid keys and cache hits.
    pub(crate) fn encode_run(
        &mut self,
        device: &wgpu::Device,
        glyphs: &mut TextGlyphs<'_>,
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
            let idx = match self.atlas.touch(&g.raster_key) {
                Some(i) => i,
                None => match rasterize_and_insert(device, glyphs, &mut self.atlas, g.raster_key) {
                    Rasterized::Slot(i) => i,
                    Rasterized::NoImage => continue,
                    Rasterized::AtlasFull => {
                        starved = true;
                        continue;
                    }
                },
            };
            let slot = self.atlas.slots[idx as usize];

            if slot.alloc.is_none() {
                continue;
            }

            let abs_x = g.x + slot.left as i32;
            let abs_y = g.y - slot.top as i32;
            let dim = RasterQuad::dim(slot.width, slot.height);
            let uv_and_kind = RasterQuad::pack_uv(slot.x, slot.y, slot.content);

            self.instances.push(RasterQuad {
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

/// Cache miss path: ask the shaper's glyph lease for the bitmap, push into
/// the atlas. A free fn, not a `TextEncoder` method: it's called while
/// `self.placed` is being iterated, so it may borrow only the disjoint
/// atlas field.
fn rasterize_and_insert(
    device: &wgpu::Device,
    glyphs: &mut TextGlyphs<'_>,
    atlas: &mut RasterAtlas<GlyphRasterKey>,
    key: GlyphRasterKey,
) -> Rasterized {
    let Some(image) = glyphs.rasterize(key) else {
        return Rasterized::NoImage;
    };
    let content = match image.kind {
        GlyphImageKind::Color => ContentType::Color,
        GlyphImageKind::Mask => ContentType::Mask,
    };
    let placement = &image.placement;
    let Some(metadata) = PackedMetadata::new(
        placement.width,
        placement.height,
        placement.left,
        placement.top,
    ) else {
        tracing::warn!(
            ?key,
            width = image.placement.width,
            height = image.placement.height,
            left = image.placement.left,
            top = image.placement.top,
            "skipping glyph raster outside packed atlas metadata range",
        );
        return Rasterized::Slot(atlas.insert_unallocated(key, content, PackedMetadata::EMPTY));
    };

    if metadata.is_empty() {
        return Rasterized::Slot(atlas.insert_unallocated(key, content, metadata));
    }
    match atlas.insert(device, key, content, metadata, &image.data) {
        Some(idx) => Rasterized::Slot(idx),
        None => Rasterized::AtlasFull,
    }
}
