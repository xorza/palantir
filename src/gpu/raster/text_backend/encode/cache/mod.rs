//! Where an encoded run is kept between frames, the arena its glyphs are packed
//! into, and the hit path that replays one.
//!
//! A replay must re-check every glyph's recorded slot generation: eviction hands
//! a slot rectangle to another glyph, and a template with the old uv would draw
//! that glyph. Growth needs no check; `etagere::grow` preserves rectangles.

use crate::common::block_arena::{BlockArena, BlockSlot};
use crate::common::expiry_wheel::ExpiryWheel;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::common::span::Span;
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::raster::raster_pass::RasterPass;
use crate::gpu::raster::text_backend::encode::{EncodedKey, EncodedRunKey};
use crate::gpu::raster::text_backend::encoded_counters::EncodedCounters;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::render::GlyphRasterKey;

#[derive(Clone, Copy, Debug)]
struct EncodedEntry {
    span: Span,
    last_use: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedGlyph {
    pub(crate) instance: RasterQuad,
    pub(crate) atlas_slot: u32,
    pub(crate) generation: u32,
}

/// The link rides in `atlas_slot`: a free block is not a glyph, so nothing reads
/// its slots until re-allocation overwrites them.
impl BlockSlot for EncodedGlyph {
    /// Four slots of slack lets a run whose glyph count shifts by one land in the class
    /// its predecessor freed; exact fit would mint a class per count.
    const GRANULE: u32 = 4;

    fn free_link(next: u32) -> Self {
        Self {
            instance: RasterQuad {
                pos: [0, 0],
                dim: [0; 2],
                size: [0; 2],
                uv_and_kind: 0,
                color: RgbaF16::TRANSPARENT,
            },
            atlas_slot: next,
            generation: 0,
        }
    }

    fn next_free(self) -> u32 {
        self.atlas_slot
    }
}

/// Age-bounded cache of encoded runs over a [`BlockArena`]. Alloc-free after
/// warmup. Why blocks rather than compaction: [the arena's
/// question](crate::common::block_arena).
#[derive(Debug)]
pub(crate) struct EncodedCache {
    map: FxHashMap<EncodedKey, EncodedEntry>,
    arena: BlockArena<EncodedGlyph>,
    /// Where [`Self::stage`] accumulates a row's glyphs before its length is known.
    /// Separate from the arena tail so the block is sized from the finished row, which
    /// lets `block_class(span.len)` recover capacity without storing it.
    pending: Vec<EncodedGlyph>,
    /// Which rows come due on which frame, so [`Self::sweep`] costs what expires
    /// ([`ExpiryWheel`]).
    expiry: ExpiryWheel<EncodedKey>,
    counters: EncodedCounters,
}

impl Default for EncodedCache {
    fn default() -> Self {
        Self {
            map: FxHashMap::default(),
            arena: BlockArena::default(),
            pending: Vec::new(),
            expiry: ExpiryWheel::with_keep(ENCODED_CACHE_KEEP_FRAMES),
            counters: EncodedCounters::default(),
        }
    }
}

impl EncodedCache {
    /// Replay `run_key`'s template into `pass`, shifted to its origin. `false` means
    /// no live template and the caller owes a full
    /// [`TextEncoder::encode_run`](super::encoder::TextEncoder::encode_run). Each
    /// instance refreshes its atlas slot's LRU stamp, so `evict_one` can't reclaim a
    /// slot this frame draws.
    pub(super) fn emit_cached(
        &mut self,
        pass: &mut RasterPass<GlyphRasterKey>,
        run_key: &EncodedRunKey,
    ) -> bool {
        let current_frame = pass.atlas.current_frame;
        let Some(entry) = self.map.get_mut(&run_key.key) else {
            return false;
        };
        let glyphs = &self.arena.slots[entry.span.range()];
        let out_start = pass.instances.len();
        pass.instances.reserve(glyphs.len());
        let mut stale = false;
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
                ..g
            });
            slot.last_use = current_frame;
        }
        if stale {
            // An eviction reused one of this run's slots, so the template is dead. Drop the
            // row now, or a y-culled run would pay the failed lookup every frame.
            if let Some(dead) = self.map.remove(&run_key.key) {
                self.arena.release(dead.span);
            }
            return false;
        }
        entry.last_use = current_frame;
        true
    }

    pub(super) fn start_row(&mut self) {
        debug_assert!(
            self.pending.is_empty(),
            "settle clears the pending row, so every encode starts empty",
        );
        self.counters.encodes.bump();
    }

    pub(super) fn stage(&mut self, glyph: EncodedGlyph) {
        self.pending.push(glyph);
    }

    /// Drop entries untouched for [`ENCODED_CACHE_KEEP_FRAMES`] frames, returning each
    /// block to its size class. The window is the constant, not an argument:
    /// [`Self::settle`] files tickets against it. Runs every frame for uniform cost; a
    /// `retain` would scale with the working set (~11 µs for 24k rows), while draining
    /// [`Self::expiry`] pays only for what came due.
    pub(super) fn sweep(&mut self, current_frame: u64) {
        let map = &mut self.map;
        let arena = &mut self.arena;
        let probe = &mut self.counters;
        // No stamp check: `last_use` only moves a deadline out, so every ticket that
        // fires is live.
        self.expiry.retire(current_frame, |key, _| {
            // Gone already: `emit_cached` drops a row whose slot was reused, leaving its ticket.
            let Entry::Occupied(slot) = map.entry(key) else {
                return None;
            };
            // A hit files no ticket; re-read the real `last_use` and re-file a live row.
            let dies_at = dies_at(slot.get().last_use);
            if dies_at > current_frame {
                probe.refiles.bump();
                return Some(dies_at);
            }
            probe.expiries.bump();
            arena.release(slot.remove().span);
            None
        });
    }

    /// Drop every encoded row. Owed by a font load: templates point at slots
    /// rasterized from the face resolved at encode time. The atlas needs no sweep; its
    /// keys carry cosmic's `font_id`, which fontdb never reuses.
    pub(super) fn clear(&mut self) {
        let arena = &mut self.arena;
        for (_, entry) in self.map.drain() {
            arena.release(entry.span);
        }
        self.expiry.clear();
    }

    /// Publish the glyphs [`Self::stage`] accumulated as `key`'s template when the
    /// encode was `complete`, else drop them. Only complete encodes become templates:
    /// `EncodedKey` carries neither bounds nor atlas occupancy, so a template with a
    /// hole would replay it forever. Incomplete cases are transient (y-culled lines
    /// return; a full atlas clears). An incomplete encode leaves any existing row intact.
    pub(super) fn settle(&mut self, key: EncodedKey, frame: u64, complete: bool) {
        // Destructured so the row is held through `map.entry` while the arena is written.
        let Self {
            map,
            arena,
            pending,
            expiry,
            ..
        } = self;
        if !complete {
            pending.clear();
            return;
        }
        match map.entry(key) {
            // Release before allocating so a re-encode reclaims its own block (a zoom or
            // width drag re-encodes the same glyph count), keeping a steady gesture from
            // growing the arena. `pending` is separate, so this aliases nothing. The
            // outstanding ticket re-files off the refreshed `last_use`.
            Entry::Occupied(mut row) => {
                arena.release(row.get().span);
                row.insert(EncodedEntry {
                    span: arena.store(pending),
                    last_use: frame,
                });
            }
            // A new row owes the wheel its first ticket; this is the only place one is filed.
            Entry::Vacant(slot) => {
                slot.insert(EncodedEntry {
                    span: arena.store(pending),
                    last_use: frame,
                });
                expiry.schedule(key, dies_at(frame));
            }
        }
        pending.clear();
    }
}

/// Frames an unused [`EncodedCache`] entry survives before
/// [`TextEncoder::end_frame`](super::encoder::TextEncoder::end_frame) sweeps it.
///
/// # Why this is below [`crate::text::RENDERED_RUN_KEEP_FRAMES`]
///
/// `EncodedKey` folds `scale_q` and `max_w_q`, so a zoom or width drag mints a
/// never-reused key per run per frame, each living the full span: `runs × (KEEP +
/// 1)` entries (~27 MB at the shaped-buffer ceiling's 120) on an arena that never
/// shrinks. 30 frames is half a second at 60 Hz; the cost is a shaper walk (the
/// buffer is still resident) for a run untouched 0.5–2 s that returns, the gain a
/// 4x smaller population. A demotion signal would be better; this is the cheap lever.
pub(super) const ENCODED_CACHE_KEEP_FRAMES: u64 = 30;

/// The frame after `last_use` on which a row is first dead, shared by
/// [`EncodedCache::settle`] and [`EncodedCache::sweep`].
const fn dies_at(last_use: u64) -> u64 {
    last_use + ENCODED_CACHE_KEEP_FRAMES + 1
}

/// A buffer must outlive the encoded entry that asks for it; crossing the
/// constants silently costs a reshape per miss.
const _: () = assert!(
    ENCODED_CACHE_KEEP_FRAMES <= RENDERED_RUN_KEEP_FRAMES,
    "the shaped-buffer window must cover the encoded-run window",
);

// Gated with the churn fixture's readers, not `internals`, which integration
// suites enable without a churn fixture.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::common::block_arena::BlockArenaCounts;
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    #[cfg(test)]
    #[cfg(test)]
    use crate::gpu::raster::text_backend::encoded_counters::EncodedCounts;
    use crate::text::key::TextShapeKey;

    /// What a unit test outside this module may ask a live cache. [`EncodedKey`]'s
    /// fields are private to `encode`, so a key only names a row across frames.
    #[cfg(test)]
    impl EncodedCache {
        pub(crate) fn rows(&self) -> usize {
            self.map.len()
        }

        pub(crate) fn arena_len(&self) -> usize {
            self.arena.slots.len()
        }

        pub(crate) fn resident_rows(&self) -> impl Iterator<Item = (EncodedKey, Span)> + '_ {
            self.map.iter().map(|(&key, entry)| (key, entry.span))
        }

        pub(crate) fn span_of(&self, key: &EncodedKey) -> Option<Span> {
            self.map.get(key).map(|entry| entry.span)
        }

        pub(crate) fn templates(&self, span: Span) -> &[EncodedGlyph] {
            &self.arena.slots[span.range()]
        }
    }

    /// Churn harness: `runs` rows re-keyed every frame, as a zoom (fresh `scale_q`) or
    /// resize drag (fresh `max_w_q`) does. Not modelled on `bins`: a pan cycles its 16
    /// values and re-hits entries.
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

        /// One gesture frame: every run mints a never-reused key and encodes; the sweep
        /// runs. Returns the resident row count.
        pub(crate) fn churn_frame(&mut self) -> usize {
            self.frame += 1;
            for run in 0..self.runs {
                for glyph in 0..self.glyphs_per_row {
                    self.cache.stage(EncodedGlyph {
                        instance: RasterQuad {
                            pos: [glyph as i32, run as i32],
                            dim: [0; 2],
                            size: [0; 2],
                            uv_and_kind: 0,
                            color: RgbaF16::TRANSPARENT,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                let key = EncodedKey {
                    text: TextShapeKey::fixture(),
                    scale_q: self.frame as u32,
                    area_color: u64::from(run),
                    bins: 0,
                };
                self.cache.settle(key, self.frame, true);
            }
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }

        #[cfg(test)]
        pub(crate) fn rows(&self) -> usize {
            self.cache.map.len()
        }

        pub(crate) const fn arena_len(&self) -> usize {
            self.cache.arena.slots.len()
        }

        #[cfg(test)]
        pub(crate) fn counts(&self) -> EncodedCounts {
            self.cache.counters.counts()
        }

        /// The arena's tallies: whether a saturated gesture still extends block storage.
        #[cfg(test)]
        pub(crate) fn block_counts(&self) -> BlockArenaCounts {
            self.cache.arena.counters.counts()
        }
    }

    /// Sweep harness for the `encoded_cache_sweep` benchmark: `rows` live rows of
    /// `glyphs_per_row`, so an iteration measures [`EncodedCache::sweep`] alone.
    #[cfg(feature = "bench")]
    #[derive(Debug, Default)]
    pub(crate) struct SweepBench {
        cache: EncodedCache,
        frame: u64,
    }

    #[cfg(feature = "bench")]
    impl SweepBench {
        /// Build one row per frame so expiry tickets land on distinct buckets as in a
        /// real scene.
        pub(crate) fn new(rows: u32, glyphs_per_row: u32) -> Self {
            let mut cache = EncodedCache::default();
            let mut frame = 0;
            for row in 0..rows {
                frame += 1;
                for glyph in 0..glyphs_per_row {
                    cache.stage(EncodedGlyph {
                        instance: RasterQuad {
                            pos: [glyph as i32, row as i32],
                            dim: [0; 2],
                            size: [0; 2],
                            uv_and_kind: 0,
                            color: RgbaF16::TRANSPARENT,
                        },
                        atlas_slot: glyph,
                        generation: 1,
                    });
                }
                // Through `settle`: it files the ticket and reserves the block.
                let key = EncodedKey {
                    text: TextShapeKey::fixture(),
                    scale_q: row,
                    area_color: 0,
                    bins: 0,
                };
                cache.settle(key, frame, true);
                // Park `last_use` beyond any frame the bench reaches: rows never expire and every
                // fired ticket re-files, the steady-state load.
                cache
                    .map
                    .get_mut(&key)
                    .expect("settle just inserted this row")
                    .last_use = u64::MAX / 2;
                // Keep the wheel's clock in step with the inserts, or far tickets clamp together.
                cache.sweep(frame);
            }
            Self { cache, frame }
        }

        /// One steady-state `end_frame` sweep: the clock advances, a few tickets re-file,
        /// nothing expires. The frame must advance per call; a repeat is a no-op under a
        /// deadline wheel. Returns the surviving row count.
        pub(crate) fn sweep_steady(&mut self) -> usize {
            self.frame += 1;
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }
    }
}

#[cfg(test)]
mod tests;
