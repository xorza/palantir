//! Where an encoded run is kept between frames, the arena its glyphs are
//! packed into, and the hit path that replays one.
//!
//! A replay must re-check every glyph's recorded slot generation before
//! it emits: eviction hands a slot rectangle to another glyph, and a
//! template holding the old uv would draw that glyph instead. Growth
//! needs no such check — `etagere::grow` preserves rectangles, so a
//! cached uv survives it.

use crate::common::block_arena::{BlockArena, BlockSlot};
use crate::common::expiry_wheel::ExpiryWheel;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::primitives::span::Span;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_pass::RasterPass;
use crate::renderer::backend::text::encode::{EncodedKey, EncodedRunKey};
use crate::renderer::backend::text::encoded_counters::EncodedCounters;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::render::GlyphRasterKey;

#[derive(Clone, Copy, Debug)]
struct EncodedEntry {
    /// Slice into [`EncodedCache::arena`] holding this run's glyph
    /// templates.
    span: Span,
    last_use: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedGlyph {
    pub(crate) instance: RasterQuad,
    pub(crate) atlas_slot: u32,
    pub(crate) generation: u32,
}

/// The link rides in `atlas_slot` because a free block is not a glyph:
/// nothing reads any field of these slots until the block is
/// re-allocated, and re-allocation overwrites them.
impl BlockSlot for EncodedGlyph {
    /// A run is a dozen to a few dozen glyphs, so four slots of slack is
    /// a small fraction of it — and the rounding is what keeps a run
    /// whose glyph count shifts by one landing in the class its
    /// predecessor freed. Exact fit would mint a class per glyph count,
    /// hundreds of them, sharpening the drift bound the module doc
    /// describes for no gain a run of this length would notice.
    const GRANULE: u32 = 4;

    fn free_link(next: u32) -> Self {
        Self {
            instance: RasterQuad {
                pos: [0, 0],
                dim: 0,
                uv_and_kind: 0,
                color: 0,
            },
            atlas_slot: next,
            generation: 0,
        }
    }

    fn next_free(self) -> u32 {
        self.atlas_slot
    }
}

/// Age-bounded cache of encoded runs over a [`BlockArena`], with each
/// `EncodedEntry` pointing at its span and each dropped row's block
/// returned to its size class. After warmup this is alloc-free — arena,
/// map, free lists and the pending buffer all retain capacity across
/// frames.
///
/// Why blocks rather than an append-only arena with a compaction step is
/// [the arena's own question](crate::common::block_arena); the numbers it
/// quotes for a 200 × 40 gesture were measured here.
#[derive(Debug)]
pub(crate) struct EncodedCache {
    map: FxHashMap<EncodedKey, EncodedEntry>,
    arena: BlockArena<EncodedGlyph>,
    /// Where [`Self::stage`] accumulates a row's glyphs before its
    /// final length is known.
    /// [`Self::settle`] either copies it into a block or drops it, so an
    /// incomplete encode costs nothing but the clear.
    ///
    /// A separate buffer rather than the arena tail: the tail is no
    /// longer a bump frontier, and sizing the block from the finished
    /// row is what lets `block_class(span.len)` recover a block's
    /// capacity later without storing it per entry.
    pending: Vec<EncodedGlyph>,
    /// Which rows come due on which frame, so [`Self::sweep`] costs what
    /// expires rather than what is resident. Runs the same
    /// file-once/re-file-on-fire protocol the shaped-buffer cache does —
    /// see [`ExpiryWheel`].
    ///
    /// This side needs it more than that one: [`Self::emit_cached`]
    /// refreshes `last_use` on *every* hit of *every* visible run, so
    /// the previous `map.retain` walked the whole table every frame
    /// purely to discover that nothing had lapsed.
    expiry: ExpiryWheel<EncodedKey>,
    /// Encode / hit / expiry / re-file / block tallies. Zero-sized
    /// outside benchmark and test builds.
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
    /// Replay `run_key`'s template into `pass`, shifted to its origin.
    /// `false` means the run has no live template and the caller owes
    /// it a full [`TextEncoder::encode_run`](super::encoder::TextEncoder::encode_run).
    ///
    /// One pass emits each instance and refreshes the backing atlas
    /// slot's LRU stamp together, so `evict_one` cannot reclaim a slot
    /// this frame still draws from.
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
            // frame until the next sweep: a re-encode only replaces it
            // if this run also survives the y-cull, so a culled run
            // would otherwise pay the failed lookup indefinitely.
            if let Some(dead) = self.map.remove(&run_key.key) {
                self.arena.release(dead.span);
            }
            return false;
        }
        entry.last_use = current_frame;
        self.counters.hits.bump();
        true
    }

    /// Open one encode, which [`Self::stage`] fills and [`Self::settle`]
    /// closes.
    pub(super) fn start_row(&mut self) {
        debug_assert!(
            self.pending.is_empty(),
            "settle clears the pending row, so every encode starts empty",
        );
        self.counters.encodes.bump();
    }

    /// Append one origin-relative template to the open encode.
    pub(super) fn stage(&mut self, glyph: EncodedGlyph) {
        self.pending.push(glyph);
    }

    /// Drop entries not touched in the last [`ENCODED_CACHE_KEEP_FRAMES`]
    /// frames, returning each dropped row's block to its size class.
    ///
    /// The window is read from the constant rather than taken as an
    /// argument, because only the constant can be right: [`Self::settle`]
    /// files each row's ticket against it, so a shorter argument would
    /// not shorten anything — the ticket simply would not fire until the
    /// constant had elapsed — and a longer one would make every ticket
    /// fire early and re-file for the difference.
    ///
    /// Runs every frame, deliberately: a cadence gate would make the
    /// cost lumpy (one frame in N paying for all of them), and uniform
    /// per-frame cost is worth more here than a lower average.
    ///
    /// A `retain` over the whole table would be uniform but uniformly
    /// proportional to the working set — a text-heavy frame paying for
    /// every resident row to discover that none had lapsed, measured at
    /// ~11 µs for 24k rows. Draining [`Self::expiry`] keeps the
    /// every-frame cadence and drops the proportionality: what a frame
    /// pays for is what came due on it.
    ///
    /// The whole pass is the drain: an expired row hands its block
    /// straight back to its free list, so there is no second traversal
    /// and nothing left for a compaction step to do.
    pub(super) fn sweep(&mut self, current_frame: u64) {
        let map = &mut self.map;
        let arena = &mut self.arena;
        let probe = &mut self.counters;
        // No stamp to check: `last_use` only ever moves a deadline out,
        // so a ticket is never supplanted and every one that fires is
        // the live one.
        self.expiry.retire(current_frame, |key, _| {
            // Gone already: `emit_cached` drops a row whose atlas slot
            // was reused, leaving its ticket behind.
            let Entry::Occupied(slot) = map.entry(key) else {
                return None;
            };
            // A hit deliberately files no ticket — that is what keeps a
            // steadily-drawn run from filing one per frame — so the real
            // `last_use` is re-read here and a live row is re-filed.
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

    /// Drop every encoded row, returning each block to its size class.
    ///
    /// What a font load owes: a row holds glyph templates pointing at
    /// atlas slots rasterized from whatever face was resolved when it was
    /// encoded, and registering a font can change that answer. The atlas
    /// itself needs no such sweep — its keys carry cosmic's `font_id`,
    /// and fontdb never reuses one.
    pub(super) fn clear(&mut self) {
        let arena = &mut self.arena;
        for (_, entry) in self.map.drain() {
            arena.release(entry.span);
        }
        self.expiry.clear();
    }

    /// Settle the glyphs [`Self::stage`] accumulated: publish them as
    /// `key`'s template when the encode was `complete`, else drop
    /// them.
    ///
    /// **Only complete encodes may become templates.** `EncodedKey`
    /// carries neither the run's bounds nor the atlas's occupancy, so a
    /// template with a hole replays that hole on every later hit and
    /// never retries — the missing glyph or line would outlive whatever
    /// caused it. Both incomplete cases are transient: a y-culled line
    /// comes back into view on the next scroll, and a glyph the atlas
    /// had no room for fits once the competing pressure clears. An
    /// incomplete encode leaves any existing row for `key` intact — its
    /// template is still valid; this attempt simply produced nothing
    /// better.
    pub(super) fn settle(&mut self, key: EncodedKey, frame: u64, complete: bool) {
        // Destructured so the row can be held through `map.entry` while
        // the arena is written beside it — one hash for the whole
        // operation instead of a probe to read the old span and a second
        // to write the new row.
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
            // Release before allocating, so a re-encode reclaims *its
            // own* block. That is the common case by far — a zoom or
            // width drag re-encodes the same text, so the row's glyph
            // count is unchanged and its old block is exactly the right
            // size class — and this order is what keeps a steady gesture
            // from growing the arena at all after warm-up. The old block
            // is unreachable from the moment the row is replaced, and
            // `pending` is a separate buffer, so handing it back before
            // the copy cannot alias anything.
            //
            // The outstanding ticket is left alone: it re-files off the
            // refreshed `last_use` when it fires.
            Entry::Occupied(mut row) => {
                arena.release(row.get().span);
                row.insert(EncodedEntry {
                    span: arena.store(pending),
                    last_use: frame,
                });
            }
            // A new row owes the wheel its first ticket, and this arm is
            // the only place one is filed — which is what makes "one
            // ticket per row, not one per encode" structural.
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

/// Frames an unused [`EncodedCache`] entry survives before being swept in
/// [`TextEncoder::end_frame`](super::encoder::TextEncoder::end_frame).
/// Keeps the cache from growing unboundedly under a long zoom gesture while
/// comfortably outliving any short flicker (visibility toggle, hover paint)
/// that drops a run for a frame.
///
/// # Why this is below [`crate::text::RENDERED_RUN_KEEP_FRAMES`]
///
/// The constraint against the shaped-buffer window is an *ordering*,
/// not an equality: a buffer has to outlive the encoded entry that
/// would come asking for it, or a miss silently pays to reshape. This
/// window being shorter satisfies that with room to spare, and the
/// `const _` assertion below is what stops a later edit from inverting
/// it.
///
/// Making the two *equal* would cost population for nothing. `EncodedKey`
/// folds `scale_q` and (through
/// [`TextShapeKey`](crate::text::key::TextShapeKey)) `max_w_q`, so a zoom
/// or width drag mints a fresh key per run per frame that will never be
/// asked for again — and with one window and no demotion signal each of
/// those lives the full span. The resident population is
/// `runs × (KEEP + 1)`, so the window *is* the population multiplier: 120
/// held 121 frames of dead gesture keys, ~27 MB of glyph templates for a
/// text-dense drag, on an arena that never shrinks.
///
/// 30 frames is half a second at 60 Hz. What it costs is a re-encode
/// for a run that goes untouched for 0.5–2 s and then comes back, which
/// is a shaper walk rather than a reshape — the buffer is still
/// resident on the longer window, which is the whole point of the
/// ordering. What it buys is a 4x smaller resident population for a
/// one-constant change, with no demotion signal to design and no new
/// way for the cache to be wrong.
///
/// The real fix for gesture churn is still a demotion signal, which
/// would cut the population to `runs × 5` regardless of this number.
/// This is the cheap lever, not a substitute for it.
pub(super) const ENCODED_CACHE_KEEP_FRAMES: u64 = 30;

/// The frame a row last used on `last_use` is first dead — what the wheel
/// files under. One expression, read by the filing in
/// [`EncodedCache::settle`] and by the re-file in [`EncodedCache::sweep`],
/// so the two cannot name different frames.
const fn dies_at(last_use: u64) -> u64 {
    last_use + ENCODED_CACHE_KEEP_FRAMES + 1
}

/// A buffer must outlive the encoded entry that would come asking for
/// it. Stated as an assertion rather than a comment because the two
/// constants now live apart, and the failure it guards is silent —
/// crossing them costs a reshape per miss and nothing reports it.
const _: () = assert!(
    ENCODED_CACHE_KEEP_FRAMES <= RENDERED_RUN_KEEP_FRAMES,
    "the shaped-buffer window must cover the encoded-run window",
);

// Gated with the churn fixture's two readers exactly — the `text_atlas`
// benchmark and the retention tests below — rather than on `internals`,
// which the two integration suites enable without ever building a churn
// fixture.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod test_support {
    use super::*;
    #[cfg(test)]
    use crate::common::block_arena::BlockArenaCounts;
    #[cfg(test)]
    use crate::common::counters::CounterSet;
    #[cfg(all(test, feature = "internals"))]
    use crate::primitives::span::Span;
    #[cfg(test)]
    use crate::renderer::backend::text::encoded_counters::EncodedCounts;
    use crate::text::key::TextShapeKey;

    /// What a unit test outside this module may ask a live cache,
    /// which is the resident population and the templates under it.
    /// A key is opaque here — [`EncodedKey`]'s fields stay private to
    /// `encode`, so it serves only to name a row across frames.
    ///
    /// Gated on `internals` too, because the GPU text tests that ask
    /// these carry that gate and nothing else in the crate asks.
    #[cfg(all(test, feature = "internals"))]
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
                for glyph in 0..self.glyphs_per_row {
                    self.cache.stage(EncodedGlyph {
                        instance: RasterQuad {
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
                self.cache.settle(key, self.frame, true);
            }
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }

        #[cfg(test)]
        pub(crate) fn rows(&self) -> usize {
            self.cache.map.len()
        }

        pub(crate) fn arena_len(&self) -> usize {
            self.cache.arena.slots.len()
        }

        #[cfg(test)]
        pub(crate) fn counts(&self) -> EncodedCounts {
            self.cache.counters.counts()
        }

        /// The *arena*'s tallies rather than the cache's — whether a
        /// saturated gesture still extends the block storage.
        #[cfg(test)]
        pub(crate) fn block_counts(&self) -> BlockArenaCounts {
            self.cache.arena.counters.counts()
        }
    }

    /// Sweep harness for the `encoded_cache_sweep` benchmark. Populates
    /// a cache with `rows` live rows of `glyphs_per_row` each — the
    /// steady-state shape a text-heavy frame leaves behind — so a
    /// benchmark iteration measures [`EncodedCache::sweep`] alone,
    /// isolated from the encode work that surrounds it in `end_frame`.
    /// Gated with that benchmark alone — unlike [`ChurnBench`], nothing
    /// here answers a question a test asks.
    #[cfg(feature = "bench")]
    #[derive(Debug, Default)]
    pub(crate) struct SweepBench {
        cache: EncodedCache,
        frame: u64,
    }

    #[cfg(feature = "bench")]
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
                for glyph in 0..glyphs_per_row {
                    cache.stage(EncodedGlyph {
                        instance: RasterQuad {
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
                // files the expiry ticket and reserves the block, and a
                // fixture missing either would measure a sweep with
                // nothing to do.
                let key = EncodedKey {
                    text: TextShapeKey::INVALID,
                    scale_q: row,
                    area_color: 0,
                    bins: 0,
                };
                cache.settle(key, frame, true);
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
                cache.sweep(frame);
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
            self.cache.sweep(self.frame);
            self.cache.map.len()
        }
    }
}

#[cfg(test)]
mod tests;
