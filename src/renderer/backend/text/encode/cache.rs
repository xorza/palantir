//! Where an encoded run is kept between frames, and the arena its glyphs are
//! packed into.

use crate::common::block_arena::{BlockArena, BlockSlot};
use crate::common::expiry_wheel::ExpiryWheel;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::text::encode::{EncodedEntry, EncodedKey};
use crate::renderer::backend::text::encoded_counters::EncodedCounters;
use crate::text::RENDERED_RUN_KEEP_FRAMES;

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
    pub(crate) map: FxHashMap<EncodedKey, EncodedEntry>,
    pub(crate) arena: BlockArena<EncodedGlyph>,
    /// Where
    /// [`TextEncoder::encode_run`](super::encoder::TextEncoder::encode_run)
    /// accumulates a row's glyphs before its final length is known.
    /// [`Self::settle`] either copies it into a block or drops it, so an
    /// incomplete encode costs nothing but the clear.
    ///
    /// A separate buffer rather than the arena tail: the tail is no
    /// longer a bump frontier, and sizing the block from the finished
    /// row is what lets `block_class(span.len)` recover a block's
    /// capacity later without storing it per entry.
    pub(crate) pending: Vec<EncodedGlyph>,
    /// Which rows come due on which frame, so [`Self::sweep`] costs what
    /// expires rather than what is resident. Runs the same
    /// file-once/re-file-on-fire protocol the shaped-buffer cache does —
    /// see [`ExpiryWheel`].
    ///
    /// This side needs it more than that one: [`TextEncoder::
    /// try_emit_cached`] refreshes `last_use` on *every* hit of *every*
    /// visible run, so the previous `map.retain` walked the whole table
    /// every frame purely to discover that nothing had lapsed.
    pub(crate) expiry: ExpiryWheel<EncodedKey>,
    /// Encode / hit / expiry / re-file / block tallies. Zero-sized
    /// outside benchmark and test builds.
    pub(crate) counters: EncodedCounters,
}

impl Default for EncodedCache {
    fn default() -> Self {
        Self {
            map: FxHashMap::default(),
            arena: BlockArena::default(),
            pending: Vec::new(),
            // `+ 2`, not `+ 1`: a ticket's deadline has to fit the ring
            // measured from the last *drained* frame, and `settle` files
            // during the frame, before `sweep` advances it. So the
            // furthest deadline is `KEEP + 1` past a `drained_through`
            // that is still one frame behind. At `KEEP = 120` the
            // power-of-two rounding hid this; at 30 it does not —
            // `KEEP + 1` rounds to exactly 32 slots, the deadline lands
            // one past the ring, and every ticket fires a frame early
            // and re-files. Correct either way, since an early ticket is
            // just a re-file, but it doubles the drain for nothing.
            expiry: ExpiryWheel::with_horizon(ENCODED_CACHE_KEEP_FRAMES + 2),
            counters: EncodedCounters::default(),
        }
    }
}

impl EncodedCache {
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
            // Gone already: `try_emit_cached` drops a row whose atlas
            // slot was reused, leaving its ticket behind.
            let Entry::Occupied(slot) = map.entry(key) else {
                return None;
            };
            // A hit deliberately files no ticket — that is what keeps a
            // steadily-drawn run from filing one per frame — so the real
            // `last_use` is re-read here and a live row is re-filed.
            let dies_at = slot.get().last_use + ENCODED_CACHE_KEEP_FRAMES + 1;
            if dies_at > current_frame {
                probe.refiles.bump();
                return Some(dies_at);
            }
            probe.expiries.bump();
            arena.release(slot.remove().span);
            None
        });
    }

    /// Settle the glyphs
    /// [`TextEncoder::encode_run`](super::encoder::TextEncoder::encode_run)
    /// accumulated in `pending`: publish them as `key`'s template when the
    /// encode was `complete`, else drop them.
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
                expiry.schedule(key, frame + ENCODED_CACHE_KEEP_FRAMES + 1);
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

/// A buffer must outlive the encoded entry that would come asking for
/// it. Stated as an assertion rather than a comment because the two
/// constants now live apart, and the failure it guards is silent —
/// crossing them costs a reshape per miss and nothing reports it.
const _: () = assert!(
    ENCODED_CACHE_KEEP_FRAMES <= RENDERED_RUN_KEEP_FRAMES,
    "the shaped-buffer window must cover the encoded-run window",
);
