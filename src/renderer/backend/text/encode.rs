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

use crate::common::expiry_wheel::ExpiryWheel;
use crate::primitives::num::F32Ext;
use crate::primitives::span::Span;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::{
    self, GlyphImageKind, GlyphRasterKey, PlacedGlyph, RunPlacement, TextRenderSession,
};
use crate::text::request::TextShapeRequest;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::renderer::backend::text::atlas::{GlyphAtlas, PackedGlyphMetadata};
use crate::renderer::backend::text::encoded_probe::EncodedProbe;
use crate::renderer::backend::text::{ContentType, GlyphInstance};

/// Cache-hit identity for an encoded run. Subpixel bins capture the
/// fractional component of `origin` that cosmic folds into per-glyph
/// `CacheKey`s (so different fractional origins produce different
/// atlas slots and can't share an entry).
///
/// `area_color` is in the key because the run's colour is baked into
/// every cached [`GlyphInstance::color`] at insert time. **This is only
/// sufficient because palantir shapes every run with one uniform
/// colour** — `attrs_for` (`cosmic.rs`) sets no per-span colour, so
/// cosmic never emits a per-glyph `color_opt`. If per-span colours are
/// ever added, fold a colour-span fingerprint into this key *first*, or
/// the cache will serve a stale run's baked colours. The assertion in
/// `TextRenderSession::extract_glyphs`'s glyph loop is the tripwire for
/// that invariant.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) struct EncodedKey {
    text: TextShapeKey,
    /// `(scale * 65536).round() as u32`. 1/65536 px is below cosmic's
    /// 4-bin subpixel resolution, so distinct quantized scales are the
    /// only ones that produce distinct cosmic cache keys.
    scale_q: u32,
    area_color: u32,
    /// Packed subpixel bins of the run origin, exactly as produced by
    /// [`crate::text::render::SubpixelOrigin::bins`].
    bins: u8,
}

/// `encode_key_for`'s named result. Carries the cache identity plus
/// the integer-pixel origin (the fractional component is folded into
/// `EncodedKey::bins`).
#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedRunKey {
    key: EncodedKey,
    origin_x: i32,
    origin_y: i32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedEntry {
    /// Slice into `EncodedCache.arena` holding this run's glyph
    /// templates.
    pub(super) span: Span,
    last_use: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedGlyph {
    instance: GlyphInstance,
    pub(super) atlas_slot: u32,
    pub(super) generation: u32,
}

/// Flat-arena cache: one contiguous `Vec<EncodedGlyph>` holds every
/// run's origin-relative glyphs, with each `EncodedEntry` pointing at
/// its span.
/// After warmup this is alloc-free — the arena/map/scratch all retain
/// capacity across frames.
#[derive(Debug)]
pub(super) struct EncodedCache {
    pub(super) map: FxHashMap<EncodedKey, EncodedEntry>,
    /// Append-only arena. Replaced runs leave dead spans behind;
    /// `sweep` compacts when dead bytes exceed live ones (see
    /// `COMPACT_RATIO`).
    pub(super) arena: Vec<EncodedGlyph>,
    /// A cache hit emits `arena` straight out without walking cosmic,
    /// so the atlas slots backing the run would never get their LRU
    /// `last_use` bumped — `evict_one` could then reclaim a slot still
    /// referenced this frame and overwrite it with a different glyph.
    /// On hit we store the current frame through each index — an
    /// indexed write, no map probe per glyph. Each encoded glyph's
    /// generation keeps the index honest when `evict_one` makes a slot
    /// reusable.
    /// Retained scratch for the compact pass — kept on the struct so
    /// compaction is a `swap`, not an alloc.
    scratch: Vec<EncodedGlyph>,
    /// Which rows come due on which frame, so [`Self::sweep`] costs what
    /// expires rather than what is resident. Runs the same
    /// file-once/re-file-on-fire protocol the shaped-buffer cache does —
    /// see [`ExpiryWheel`].
    ///
    /// This side needs it more than that one: [`TextEncoder::
    /// try_emit_cached`] refreshes `last_use` on *every* hit of *every*
    /// visible run, so the previous `map.retain` walked the whole table
    /// every frame purely to discover that nothing had lapsed.
    expiry: ExpiryWheel<EncodedKey>,
    /// Glyphs reachable through some live row — the compaction trigger's
    /// denominator.
    ///
    /// Maintained incrementally because the sweep no longer visits every
    /// row and so can no longer total it for free. Every path that adds,
    /// replaces, or drops a row adjusts it; [`Self::settle`] is the only
    /// one that adds.
    live_glyphs: usize,
    /// Encode / hit / expiry / re-file tallies. Zero-sized outside
    /// benchmark and test builds.
    pub(super) probe: EncodedProbe,
}

impl Default for EncodedCache {
    fn default() -> Self {
        Self {
            map: FxHashMap::default(),
            arena: Vec::new(),
            scratch: Vec::new(),
            expiry: ExpiryWheel::with_horizon(ENCODED_CACHE_KEEP_FRAMES + 1),
            live_glyphs: 0,
            probe: EncodedProbe::default(),
        }
    }
}

/// Compact when `arena.len() > live_glyphs * (1 + COMPACT_RATIO)`,
/// i.e. dead glyphs exceed 50% of live ones. Tuned to amortize the
/// compact cost over many frames while bounding wasted memory.
const COMPACT_RATIO: usize = 1;

impl EncodedCache {
    /// Drop entries not touched in the last `keep_frames` frames and,
    /// when the arena holds more dead-glyph slack than live, compact it
    /// into the retained scratch. Compaction rewrites every surviving
    /// entry's `span`.
    ///
    /// Runs every frame, deliberately: a cadence gate would make the
    /// cost lumpy (one frame in N paying for all of them), and uniform
    /// per-frame cost is worth more here than a lower average.
    ///
    /// It used to be a `retain` over the whole table, which is uniform
    /// but uniformly proportional to the working set — a text-heavy
    /// frame paid for every resident row to discover that none had
    /// lapsed, and audit F6 measured ~11 µs at 24k rows. Draining
    /// [`Self::expiry`] keeps the every-frame cadence and drops the
    /// proportionality: what a frame pays for is what came due on it.
    ///
    /// One traversal per frame is still the rule — hence
    /// [`Self::live_glyphs`] carrying the compaction denominator across
    /// frames, since the survivors are no longer all in hand here to
    /// total.
    fn sweep(&mut self, current_frame: u64, keep_frames: u64) {
        let map = &mut self.map;
        let live_glyphs = &mut self.live_glyphs;
        let probe = &mut self.probe;
        self.expiry.retire(current_frame, |key| {
            // Gone already: `try_emit_cached` drops a row whose atlas
            // slot was reused, leaving its ticket behind.
            let Entry::Occupied(slot) = map.entry(key) else {
                return None;
            };
            // A hit deliberately files no ticket — that is what keeps a
            // steadily-drawn run from filing one per frame — so the real
            // `last_use` is re-read here and a live row is re-filed.
            let dies_at = slot.get().last_use + keep_frames + 1;
            if dies_at > current_frame {
                probe.refiles.bump();
                return Some(dies_at);
            }
            probe.expiries.bump();
            *live_glyphs -= slot.remove().span.len as usize;
            None
        });

        if self.arena.len() <= self.live_glyphs * (1 + COMPACT_RATIO) {
            return;
        }
        self.scratch.clear();
        for entry in self.map.values_mut() {
            let new_start = self.scratch.len() as u32;
            let r = entry.span.range();
            self.scratch.extend_from_slice(&self.arena[r]);
            entry.span = Span::new(new_start, entry.span.len);
        }
        std::mem::swap(&mut self.arena, &mut self.scratch);
    }

    /// Settle the glyphs [`TextEncoder::encode_run`] appended since
    /// `pending_start`: publish them as `key`'s template when the encode
    /// was `complete`, else roll them back off the arena.
    ///
    /// **Only complete encodes may become templates.** `EncodedKey`
    /// carries neither the run's bounds nor the atlas's occupancy, so a
    /// template with a hole replays that hole on every later hit and
    /// never retries — the missing glyph or line would outlive whatever
    /// caused it. Both incomplete cases are transient: a y-culled line
    /// comes back into view on the next scroll, and a glyph the atlas
    /// had no room for fits once the competing pressure clears.
    fn settle(&mut self, key: EncodedKey, pending_start: u32, frame: u64, complete: bool) {
        if !complete {
            self.arena.truncate(pending_start as usize);
            return;
        }
        let span = Span::new(pending_start, self.arena.len() as u32 - pending_start);
        self.live_glyphs += span.len as usize;
        let replaced = self.map.insert(
            key,
            EncodedEntry {
                span,
                last_use: frame,
            },
        );
        match replaced {
            // A re-encode of a live row orphans its old span on the
            // arena; only the new one is reachable.
            Some(old) => self.live_glyphs -= old.span.len as usize,
            // First ticket for this row. A replacement keeps the
            // outstanding one, which re-files off the refreshed
            // `last_use` when it fires.
            None => self
                .expiry
                .schedule(key, frame + ENCODED_CACHE_KEEP_FRAMES + 1),
        }
    }
}

/// Build the cache key for a `TextDrawRow` placed at `frame_scale * r.scale`,
/// plus the integer-pixel origin (cosmic's subpixel bins absorb the
/// fractional component into per-glyph `CacheKey`s, so two runs at
/// different fractional origins live in different cache entries).
pub(super) fn encode_key_for(r: &TextDrawRow, frame_scale: f32) -> EncodedRunKey {
    let scale = frame_scale * r.scale;
    let area_color: u32 = bytemuck::cast(r.color);
    let sub = render::subpixel_origin(r.origin);
    EncodedRunKey {
        key: EncodedKey {
            text: r.text.key,
            scale_q: (scale * 65536.0).fast_round() as u32,
            area_color,
            bins: sub.bins,
        },
        origin_x: sub.x,
        origin_y: sub.y,
    }
}

/// Frames an unused [`EncodedCache`] entry survives before being swept
/// in [`TextEncoder::end_frame`]. Keeps the cache from growing
/// unboundedly under a long zoom gesture while comfortably outliving
/// any short flicker (visibility toggle, hover paint) that drops a run
/// for a frame.
/// Sourced from [`crate::text::RENDERED_RUN_KEEP_FRAMES`], which the
/// shaped-buffer cache's protected window derives from too — the two
/// have to agree, so they are one value.
const ENCODED_CACHE_KEEP_FRAMES: u64 = crate::text::RENDERED_RUN_KEEP_FRAMES;

/// CPU-side glyph encoder: owns the atlas, the encoded-run cache, the
/// per-miss extraction scratch, and the frame's accumulated instances.
/// `TextBackend` owns one and partitions `instances` into per-batch
/// draw ranges; owning the state here lets every method borrow
/// disjoint fields directly, with no per-call context bundle.
#[derive(Debug)]
pub(super) struct TextEncoder {
    pub(super) atlas: GlyphAtlas,
    pub(super) cache: EncodedCache,
    /// Retained per-miss extraction scratch.
    placed: Vec<PlacedGlyph>,
    /// Drawable glyph instances accumulated across this frame's
    /// batches.
    pub(super) instances: Vec<GlyphInstance>,
    /// Whether a run this frame hit a full atlas, and whether that has
    /// been reported since the last frame that didn't.
    ///
    /// Starvation is not corruption — the glyph is skipped, the run is
    /// refused as a template, and it re-encodes next frame — but it is
    /// silent, self-inflicted slowness with a visible hole in the text,
    /// and nothing else in the pipeline would say so. Edge-triggered
    /// because it recurs per glyph per run per frame; logging each one
    /// would bury the signal in its own noise.
    starved_this_frame: bool,
    starved_reported: bool,
}

impl TextEncoder {
    pub(super) fn new(device: &wgpu::Device) -> Self {
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
    pub(super) fn try_emit_cached(&mut self, run_key: &EncodedRunKey) -> bool {
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
                self.cache.live_glyphs -= dead.span.len as usize;
            }
            return false;
        }
        entry.last_use = current_frame;
        self.cache.probe.hits.bump();
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
    pub(super) fn end_frame(&mut self, frame: u64) {
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
    pub(super) fn encode_run(
        &mut self,
        device: &wgpu::Device,
        session: &mut TextRenderSession<'_>,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        run_key: EncodedRunKey,
    ) {
        let current_frame = self.atlas.current_frame;
        self.cache.probe.encodes.bump();
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
        let pending_start = self.cache.arena.len() as u32;

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
            self.cache.arena.push(EncodedGlyph {
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
        self.cache
            .settle(run_key.key, pending_start, current_frame, complete);
    }
}

/// Pack `(u, v, kind)` into the 32-bit `uv_and_kind` field. `u`'s
/// high bit carries `content_type` (atlases cap at 16384 = 14 bits).
#[inline]
fn pack_uv(u: u16, v: u16, kind: ContentType) -> u32 {
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

// Wider than `feature = "internals"`: `ChurnBench` is read by the
// `text_atlas` benchmark *and* by the retention test below, which builds
// under bare `cfg(test)`. `pub(super)` reaches both — the benchmark's
// caller lives in this module's sibling `bench.rs`, not outside the text
// backend.
#[cfg(any(test, feature = "internals"))]
pub(super) mod internals {
    #![allow(dead_code)]
    use super::*;
    #[cfg(test)]
    use crate::renderer::backend::text::encoded_probe::EncodedCounts;

    /// Churn harness: `runs` rows **re-keyed every frame**, which is what
    /// a zoom (a fresh `scale_q` per `TEXT_SCALE_STEP` rung) or a resize
    /// drag (a fresh `max_w_q` per committed width) produces.
    ///
    /// Not modelled on `bins`: that component takes only 16 values and a
    /// pan cycles back through them, so sub-pixel motion re-hits its
    /// entries instead of minting new ones.
    #[derive(Debug, Default)]
    pub(crate) struct ChurnBench {
        cache: EncodedCache,
        frame: u64,
        runs: u32,
        glyphs_per_row: u32,
    }

    impl ChurnBench {
        pub(crate) fn new(runs: u32, glyphs_per_row: u32) -> Self {
            Self {
                cache: EncodedCache::default(),
                frame: 0,
                runs,
                glyphs_per_row,
            }
        }

        /// One gesture frame: every run mints a key it will never be
        /// asked for again, encodes its glyphs into the arena, and the
        /// sweep runs. Returns the resident row count.
        pub(crate) fn churn_frame(&mut self) -> usize {
            self.frame += 1;
            for run in 0..self.runs {
                let start = self.cache.arena.len() as u32;
                for glyph in 0..self.glyphs_per_row {
                    self.cache.arena.push(EncodedGlyph {
                        instance: GlyphInstance {
                            pos: [glyph as i32, run as i32],
                            dim: 0,
                            uv_and_kind: 0,
                            color: 0,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                let key = EncodedKey {
                    text: TextShapeKey::INVALID,
                    // The churn axis: one fresh rung per frame.
                    scale_q: self.frame as u32,
                    // Run identity, stable across the gesture.
                    area_color: run,
                    bins: 0,
                };
                self.cache.settle(key, start, self.frame, true);
            }
            self.cache.sweep(self.frame, ENCODED_CACHE_KEEP_FRAMES);
            self.cache.map.len()
        }

        pub(crate) fn rows(&self) -> usize {
            self.cache.map.len()
        }

        pub(crate) fn arena_len(&self) -> usize {
            self.cache.arena.len()
        }

        #[cfg(test)]
        pub(crate) fn counts(&self) -> EncodedCounts {
            self.cache.probe.counts()
        }
    }

    /// Sweep harness for the `encoded_cache_sweep` benchmark. Populates
    /// a cache with `rows` live rows of `glyphs_per_row` each — the
    /// steady-state shape a text-heavy frame leaves behind — so a
    /// benchmark iteration measures [`EncodedCache::sweep`] alone,
    /// isolated from the encode work that surrounds it in `end_frame`.
    #[derive(Debug, Default)]
    pub(crate) struct SweepBench {
        cache: EncodedCache,
        frame: u64,
    }

    impl SweepBench {
        /// Build `rows` rows **one per frame**, so their expiry tickets
        /// land on distinct buckets exactly as a real scene's inserts
        /// do.
        ///
        /// Populating them all on one frame would be easier and wrong:
        /// every ticket would share a bucket, and the measurement would
        /// alternate between frames that drain nothing and one frame in
        /// a ring that drains everything — a burst the fixture invented,
        /// not one the cache produces.
        pub(crate) fn new(rows: u32, glyphs_per_row: u32) -> Self {
            let mut cache = EncodedCache::default();
            let mut frame = 0;
            for row in 0..rows {
                frame += 1;
                let start = cache.arena.len() as u32;
                for glyph in 0..glyphs_per_row {
                    cache.arena.push(EncodedGlyph {
                        instance: GlyphInstance {
                            pos: [glyph as i32, row as i32],
                            dim: 0,
                            uv_and_kind: 0,
                            color: 0,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                // Through `settle`, not by poking the map: it is what
                // files the expiry ticket and tracks the live-glyph
                // total, and a fixture missing either would measure a
                // sweep with nothing to do.
                let key = EncodedKey {
                    text: TextShapeKey::INVALID,
                    scale_q: row,
                    area_color: 0,
                    bins: 0,
                };
                cache.settle(key, start, frame, true);
                // Park `last_use` beyond any frame the bench reaches, so
                // rows never expire and every fired ticket is re-filed.
                // That is the steady-state load: a drawn run refreshes
                // `last_use` on the encoded-cache hit path and files
                // nothing, so the sweep's whole job is re-filing.
                cache
                    .map
                    .get_mut(&key)
                    .expect("settle just inserted this row")
                    .last_use = u64::MAX / 2;
                // Keep the wheel's clock in step with the inserts, or
                // tickets more than a ring out get clamped together and
                // the stagger is lost before the bench starts.
                cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
            }
            Self { cache, frame }
        }

        /// One steady-state `end_frame` sweep: the clock advances, the
        /// handful of tickets that came due are re-filed, and nothing
        /// expires — exactly the pass a frame pays when the cache is
        /// warm and every row is still on screen. Returns the surviving
        /// row count so the caller can assert the fixture stayed intact.
        ///
        /// The frame *must* advance per call. Sweeping the same frame
        /// twice is a no-op under a deadline wheel, so a fixed-frame
        /// harness would measure an early return and guard nothing.
        pub(crate) fn sweep_steady(&mut self) -> usize {
            self.frame += 1;
            self.cache.sweep(self.frame, ENCODED_CACHE_KEEP_FRAMES);
            self.cache.map.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::backend::text::encode::{ContentType, pack_uv};

    fn key(scale_q: u32) -> EncodedKey {
        EncodedKey {
            text: TextShapeKey::INVALID,
            scale_q,
            area_color: 0,
            bins: 0,
        }
    }

    /// Distinguishable glyph payload — `tag` reaches every field so a
    /// compaction that shuffled or truncated data can't pass.
    fn glyph(tag: u32) -> EncodedGlyph {
        EncodedGlyph {
            instance: GlyphInstance {
                pos: [tag as i32, -(tag as i32)],
                dim: tag,
                uv_and_kind: tag << 8,
                color: !tag,
            },
            atlas_slot: tag,
            generation: tag + 1,
        }
    }

    /// Byte-exact comparison: `GlyphInstance` is `Pod`, so this catches
    /// any field the copy dropped.
    fn same(a: &EncodedGlyph, b: &EncodedGlyph) -> bool {
        bytemuck::bytes_of(&a.instance) == bytemuck::bytes_of(&b.instance)
            && a.atlas_slot == b.atlas_slot
            && a.generation == b.generation
    }

    /// Push `glyphs` onto the arena and point `k` at them, as a
    /// re-encode of that run would.
    fn insert(cache: &mut EncodedCache, k: EncodedKey, tags: impl Iterator<Item = u32>, at: u64) {
        let start = cache.arena.len() as u32;
        cache.arena.extend(tags.map(glyph));
        cache.settle(k, start, at, true);
    }

    /// Sweeping every frame makes the retention window exact: a row
    /// unused since frame `L` is kept while `L >= frame - KEEP` and dies
    /// on the first frame past that, i.e. at `L + KEEP + 1` — so its
    /// lifetime is exactly KEEP + 1 frames regardless of when it was
    /// last touched. Two offsets pin that the death frame tracks `L`
    /// rather than landing on some grid.
    #[test]
    fn unused_rows_die_one_frame_past_the_keep_window() {
        for last_use in [0u64, 9] {
            let mut cache = EncodedCache::default();
            insert(&mut cache, key(1), 0..1, last_use);
            let mut died = None;
            for frame in last_use + 1..=last_use + 400 {
                cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
                if cache.map.is_empty() {
                    died = Some(frame);
                    break;
                }
            }
            assert_eq!(
                died,
                Some(last_use + ENCODED_CACHE_KEEP_FRAMES + 1),
                "row unused since {last_use}",
            );
        }
    }

    /// Sweeping every frame keeps the arena bounded at all times: dead
    /// spans pile up only until they exceed the live glyphs, then
    /// compaction reclaims them, so the arena never exceeds
    /// `live * (1 + COMPACT_RATIO)` by more than one frame's appends.
    /// Hand-traced with a 10-glyph untouched row plus a 4-glyph run
    /// re-encoded every frame — COMPACT_RATIO = 1 puts the threshold at
    /// `arena > live * 2` = 28:
    ///
    /// frame 1 → 14, 2 → 18, 3 → 22, 4 → 26 (all within 28, kept),
    /// frame 5 → 30 > 28 → compacts back to the 14 live glyphs.
    ///
    /// Then exactness: every survivor comes out byte-identical with its
    /// span rewritten. Row order in the compacted arena follows map
    /// iteration order, so each row is read through its own span and the
    /// two spans must tile the arena.
    #[test]
    fn arena_compacts_as_soon_as_dead_glyphs_exceed_live() {
        let mut cache = EncodedCache::default();
        // Untouched row: 10 glyphs, well inside its keep window.
        insert(&mut cache, key(1), 1000..1010, 0);
        for (frame, expected_arena) in [(1u64, 14), (2, 18), (3, 22), (4, 26), (5, 14)] {
            let base = frame as u32 * 10;
            insert(&mut cache, key(2), base..base + 4, frame);
            cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
            assert_eq!(cache.arena.len(), expected_arena, "after frame {frame}");
        }
        assert_eq!(cache.map.len(), 2, "neither row is past its keep window");

        let untouched = cache.map[&key(1)].span;
        let churned = cache.map[&key(2)].span;
        assert_eq!(untouched.len, 10);
        assert_eq!(churned.len, 4);
        assert!(
            untouched.range().end == churned.range().start
                || churned.range().end == untouched.range().start,
            "surviving spans must tile the compacted arena: {untouched:?} / {churned:?}",
        );
        for (span, tags) in [(untouched, 1000..1010), (churned, 50..54)] {
            for (got, want) in cache.arena[span.range()].iter().zip(tags.map(glyph)) {
                assert!(same(got, &want), "compaction altered a glyph: {got:?}");
            }
        }
    }

    /// An incomplete encode leaves nothing behind: no map row for the
    /// key, and no dead glyphs on the arena. Both incomplete cases (a
    /// y-culled line, an atlas with no room) reach `settle` as the same
    /// `complete: false`, so one table covers them.
    ///
    /// The negative half is the one that matters: caching a short run
    /// would replay its hole forever, since the key records neither the
    /// bounds nor the atlas occupancy that produced it.
    #[test]
    fn only_complete_encodes_become_templates() {
        for (complete, expect_rows) in [(true, 1), (false, 0)] {
            let mut cache = EncodedCache::default();
            // A prior run's template — must survive either outcome.
            insert(&mut cache, key(1), 100..103, 7);
            let pending_start = cache.arena.len() as u32;
            cache.arena.extend((200..202).map(glyph));

            cache.settle(key(2), pending_start, 9, complete);

            assert_eq!(
                cache.map.contains_key(&key(2)),
                complete,
                "complete = {complete}",
            );
            assert_eq!(cache.map.len(), 1 + expect_rows, "complete = {complete}");
            assert_eq!(
                cache.arena.len(),
                if complete { 5 } else { 3 },
                "rolled-back glyphs must not linger on the arena",
            );
            let survivor = cache.map[&key(1)].span;
            for (got, want) in cache.arena[survivor.range()]
                .iter()
                .zip((100..103).map(glyph))
            {
                assert!(same(got, &want), "settle disturbed a live row: {got:?}");
            }
            if complete {
                let span = cache.map[&key(2)].span;
                assert_eq!((span.start, span.len), (pending_start, 2));
                assert_eq!(cache.map[&key(2)].last_use, 9);
            }
        }
    }

    /// The property the wheel exists for: a sweep costs what expires,
    /// not what is resident.
    ///
    /// A steadily-drawn row refreshes `last_use` every frame and files
    /// nothing; its one outstanding ticket fires once a window, finds it
    /// live, and re-files. Filing on every touch instead would still
    /// expire correctly, but would hold `rows × KEEP` tickets and drain
    /// `rows` of them per frame — the whole-table walk this replaced,
    /// wearing a different hat.
    #[test]
    fn a_steadily_drawn_row_holds_one_ticket_not_one_per_frame() {
        const ROWS: u32 = 50;
        let mut cache = EncodedCache::default();
        for row in 0..ROWS {
            insert(&mut cache, key(row), 0..4, 0);
        }
        assert_eq!(cache.expiry.pending(), ROWS as usize, "one ticket each");

        for frame in 1..=ENCODED_CACHE_KEEP_FRAMES * 3 {
            for row in 0..ROWS {
                cache
                    .map
                    .get_mut(&key(row))
                    .expect("a drawn row stays resident")
                    .last_use = frame;
            }
            cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
        }

        assert_eq!(cache.map.len(), ROWS as usize, "every row is still live");
        assert_eq!(
            cache.expiry.pending(),
            ROWS as usize,
            "three windows of redraw must not accumulate tickets",
        );
        assert_eq!(
            cache.live_glyphs,
            ROWS as usize * 4,
            "the incrementally-tracked live total must match the rows",
        );

        // And they still die once the redraw stops — the re-filing did
        // not push the deadline out of reach.
        let last = ENCODED_CACHE_KEEP_FRAMES * 3;
        for frame in last + 1..=last + ENCODED_CACHE_KEEP_FRAMES + 1 {
            cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
        }
        assert!(cache.map.is_empty(), "rows outlived their window");
        assert_eq!(cache.live_glyphs, 0, "live total must return to zero");
    }

    /// Sizes the problem a probation tier would solve, so the tier can
    /// be argued from a number instead of a hunch.
    ///
    /// A zoom or resize drag re-keys every visible run every frame, and
    /// each of those keys is asked for exactly once — the gesture has
    /// moved on by the next frame. With one window and no demotion they
    /// nonetheless live the full `ENCODED_CACHE_KEEP_FRAMES`, so the
    /// resident population settles at `runs × (KEEP + 1)`: eight visible
    /// runs cost 968 rows and ~12k glyph templates for two seconds after
    /// the drag ends.
    ///
    /// What it also shows is where the cost *isn't*. Every one of those
    /// rows is a single-use key, so its ticket fires once and expires —
    /// `refiles` stays zero and the sweep never re-walks them. The wheel
    /// already handles this shape; what is left is the population itself
    /// and the arena compaction it drives.
    #[test]
    fn a_gesture_frame_retains_a_full_keep_window_of_single_use_rows() {
        const RUNS: u32 = 8;
        const GLYPHS: u32 = 12;
        let mut churn = internals::ChurnBench::new(RUNS, GLYPHS);

        // Run past the window so the population reaches steady state.
        const FRAMES: u64 = ENCODED_CACHE_KEEP_FRAMES * 2;
        for _ in 0..FRAMES {
            churn.churn_frame();
        }

        // Rows minted on frames `F - KEEP ..= F` are all still resident.
        let window = ENCODED_CACHE_KEEP_FRAMES as usize + 1;
        assert_eq!(
            churn.rows(),
            RUNS as usize * window,
            "a drag holds every run's key for the whole keep window",
        );

        let counts = churn.counts();
        assert_eq!(
            counts.refiles, 0,
            "single-use keys are never re-filed — the drain is not the cost here",
        );
        // Everything minted and no longer resident has expired — the
        // population is bounded, just far above what the gesture uses.
        let minted = RUNS * FRAMES as u32;
        assert_eq!(counts.encodes, 0, "the fixture inserts below `encode_run`");
        assert_eq!(
            counts.expiries as usize,
            minted as usize - churn.rows(),
            "steady state expires everything it mints beyond the window",
        );
        assert!(
            churn.arena_len() >= churn.rows() * GLYPHS as usize,
            "every resident row's glyphs are still on the arena",
        );
    }

    #[test]
    fn pack_uv_round_trip() {
        let p = pack_uv(12345, 54321, ContentType::Color);
        assert_eq!(p & 0x7FFF, 12345);
        assert_eq!((p >> 15) & 1, 1);
        assert_eq!(p >> 16, 54321);

        let p = pack_uv(12345, 54321, ContentType::Mask);
        assert_eq!((p >> 15) & 1, 0);
    }
}
