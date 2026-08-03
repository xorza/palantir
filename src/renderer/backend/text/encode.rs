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

use crate::primitives::num::F32Ext;
use crate::primitives::span::Span;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::key::TextShapeKey;
use crate::text::render::{
    self, GlyphImageKind, GlyphRasterKey, PlacedGlyph, RunPlacement, TextRenderSession,
};
use crate::text::request::TextShapeRequest;
use rustc_hash::FxHashMap;

use crate::renderer::backend::text::atlas::{GlyphAtlas, PackedGlyphMetadata};
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
#[derive(Debug, Default)]
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
    /// per-frame cost is worth more here than a lower average — the
    /// whole pass is ~1.6 ns per live row (`encoded_cache_sweep` bench),
    /// so there is no burst worth amortizing into a spike.
    ///
    /// The live-glyph total is accumulated *inside* the expiry `retain`
    /// rather than by a second `values()` pass: the survivors are
    /// already in hand there, and the compaction test needs nothing
    /// else. One traversal per frame instead of two.
    fn sweep(&mut self, current_frame: u64, keep_frames: u64) {
        let cutoff = current_frame.saturating_sub(keep_frames);
        let mut live = 0usize;
        self.map.retain(|_, e| {
            let keep = e.last_use >= cutoff;
            if keep {
                live += e.span.len as usize;
            }
            keep
        });

        if self.arena.len() <= live * (1 + COMPACT_RATIO) {
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
        self.map.insert(
            key,
            EncodedEntry {
                span,
                last_use: frame,
            },
        );
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
}

impl TextEncoder {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            atlas: GlyphAtlas::new(device),
            cache: EncodedCache::default(),
            placed: Vec::new(),
            instances: Vec::new(),
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
            self.cache.map.remove(&run_key.key);
            return false;
        }
        entry.last_use = current_frame;
        true
    }

    /// Frame teardown: age the atlas LRU and sweep both caches.
    pub(super) fn end_frame(&mut self) {
        self.atlas.end_frame();
        self.cache
            .sweep(self.atlas.current_frame, ENCODED_CACHE_KEEP_FRAMES);
        self.instances.clear();
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

// `internals` only, not `any(test, …)`: the sole consumer is the
// `text_atlas` benchmark, which that feature gates too.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use super::*;

    /// Sweep harness for the `encoded_cache_sweep` benchmark. Populates
    /// a cache with `rows` live rows of `glyphs_per_row` each — the
    /// steady-state shape a text-heavy frame leaves behind — so a
    /// benchmark iteration measures [`EncodedCache::sweep`] alone,
    /// isolated from the encode work that surrounds it in `end_frame`.
    #[derive(Debug, Default)]
    pub(crate) struct SweepBench {
        cache: EncodedCache,
    }

    impl SweepBench {
        pub(crate) fn new(rows: u32, glyphs_per_row: u32) -> Self {
            let mut cache = EncodedCache::default();
            for row in 0..rows {
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
                cache.map.insert(
                    EncodedKey {
                        text: TextShapeKey::INVALID,
                        scale_q: row,
                        area_color: 0,
                        bins: 0,
                    },
                    EncodedEntry {
                        span: Span::new(start, glyphs_per_row),
                        last_use: ENCODED_CACHE_KEEP_FRAMES,
                    },
                );
            }
            Self { cache }
        }

        /// One steady-state `end_frame` sweep: the cutoff lands before
        /// every row's stamp, so nothing expires and the arena holds no
        /// dead spans — exactly the pass a frame pays when the cache is
        /// warm and nothing changed. Returns the surviving row count so
        /// the caller can assert the fixture stayed intact.
        pub(crate) fn sweep_steady(&mut self) -> usize {
            self.cache
                .sweep(ENCODED_CACHE_KEEP_FRAMES, ENCODED_CACHE_KEEP_FRAMES);
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
        let len = cache.arena.len() as u32 - start;
        cache.map.insert(
            k,
            EncodedEntry {
                span: Span::new(start, len),
                last_use: at,
            },
        );
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
