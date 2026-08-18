//! Where an encoded run is kept between frames, and the arena its glyphs are
//! packed into.

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
use crate::primitives::span::Span;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::renderer::backend::text::GlyphInstance;
use crate::renderer::backend::text::encode::{EncodedEntry, EncodedKey};
use crate::renderer::backend::text::encoded_counters::EncodedCounters;

/// End of a size class's free list. Distinguishable from every real
/// block start: a start is an index into the arena, which is bounded by
/// the glyph population, and `u32::MAX` slots of `EncodedGlyph` is 112 GB.
pub(super) const NIL: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodedGlyph {
    pub(crate) instance: GlyphInstance,
    pub(crate) atlas_slot: u32,
    pub(crate) generation: u32,
}

impl EncodedGlyph {
    /// A freed block with `next` as the following block in its size
    /// class.
    ///
    /// The free list is **intrusive**: a block's first slot holds the
    /// link, so the per-class index is a single `u32` head rather than a
    /// `Vec` per class — no second allocation, no pointer chase off to
    /// the side, and the link lands in the very cache line
    /// [`EncodedCache::alloc_block`] is about to hand back. It rides in
    /// `atlas_slot` because a free block is not a glyph: nothing reads
    /// any field of these slots until the block is re-allocated, and
    /// re-allocation overwrites them.
    ///
    /// Doubles as the fill for the slack slots of a freshly extended
    /// block, which are equally never read — a row's `span.len` covers
    /// only the glyphs actually written.
    const fn free_link(next: u32) -> Self {
        Self {
            instance: GlyphInstance {
                pos: [0, 0],
                dim: 0,
                uv_and_kind: 0,
                color: 0,
            },
            atlas_slot: next,
            generation: 0,
        }
    }

    /// The link out of a free block — see [`Self::free_link`].
    const fn next_free(self) -> u32 {
        self.atlas_slot
    }
}

/// Slot granularity of an arena block. A row's storage is rounded up to
/// a multiple of this, and a freed block is reusable only by a row in
/// the same size class — which is what lets a block be handed back and
/// taken again without ever moving anything.
///
/// The rounding is what buys that: exact-fit lists would recycle
/// perfectly for the workload that matters (a zoom or width drag
/// re-encodes the *same text*, so a run's glyph count is unchanged
/// frame to frame) and strand a block the moment a length shifted by
/// one. Four slots is 112 bytes of slack per row worst case, against
/// the 28-byte glyphs it is rounding.
const BLOCK_GRANULE: u32 = 4;

/// Size class of a row of `len` glyphs. `len` must be non-zero — a
/// glyphless row stores nothing and never reaches the allocator.
#[inline]
fn block_class(len: u32) -> usize {
    debug_assert!(len > 0, "a glyphless row is not allocated");
    ((len - 1) / BLOCK_GRANULE) as usize
}

/// Slots a block of `class` holds. The inverse of [`block_class`] on the
/// class boundary, which is what makes `free_block` able to recover a
/// block's capacity from the row length alone — no per-entry field.
#[inline]
fn block_capacity(class: usize) -> u32 {
    (class as u32 + 1) * BLOCK_GRANULE
}

/// Block-allocated cache: one `Vec<EncodedGlyph>` arena carved into
/// size-classed blocks, with each `EncodedEntry` pointing at its span
/// and each freed block returned to a per-class free list.
/// After warmup this is alloc-free — arena, map, free lists and the
/// pending buffer all retain capacity across frames.
///
/// # Why blocks rather than an append-only arena
///
/// Appending every encode to the arena tail and leaving the replaced
/// span behind as dead space means compacting once dead exceeds live.
/// Compaction copies *every live glyph* in a single frame, and
/// under a gesture — where each frame appends one frame's worth and
/// expires one frame's worth, so live stays flat — the trigger fires on
/// a fixed period of `⌊live / appends-per-frame⌋ + 1`, which for pure
/// churn is exactly 122 frames whatever the run and glyph counts.
/// Measured on `ChurnBench`, median frame against the compaction frame:
///
/// ```text
///   runs × glyphs   live glyphs   median   compaction   ratio
///           8 × 12        11 616   0.7 µs        19 µs     28×
///          50 × 25       151 250   3.0 µs       271 µs     91×
///         200 × 40       968 000    21 µs      2520 µs    120×
/// ```
///
/// Amortised that is free — the copy per frame averages exactly one
/// frame's appends — but 2.5 ms landing on one frame in 122 is a
/// dropped frame, and "uniform per-frame cost is worth more than a
/// lower average" is the rule this module already states for its sweep.
/// Recycling blocks in place removes the copy entirely instead of
/// spreading it: nothing is ever relocated, so no row's `span` is ever
/// rewritten.
#[derive(Debug)]
pub(crate) struct EncodedCache {
    pub(crate) map: FxHashMap<EncodedKey, EncodedEntry>,
    /// Block storage. Grows to the working set's high-water mark and is
    /// then reused in place; never compacted, so a live row's `span` is
    /// stable for the row's whole life.
    pub(crate) arena: Vec<EncodedGlyph>,
    /// Head of each size class's intrusive free list, `NIL` when the
    /// class is empty — `free_heads[c]` starts a chain of blocks of
    /// `block_capacity(c)` slots, linked through their first slot.
    /// LIFO, so the block handed out is the one most recently freed and
    /// therefore the one most likely still in cache.
    ///
    /// Flat by construction: one `u32` per class, and the chain itself
    /// costs nothing because it lives in space that is already free.
    pub(crate) free_heads: Vec<u32>,
    /// Where [`TextEncoder::encode_run`] accumulates a row's glyphs
    /// before its final length is known. [`Self::settle`] either copies
    /// it into a block or drops it, so an incomplete encode costs
    /// nothing but the clear.
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
            arena: Vec::new(),
            free_heads: Vec::new(),
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
    /// Drop entries not touched in the last `keep_frames` frames,
    /// returning each dropped row's block to its size class.
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
    pub(super) fn sweep(&mut self, current_frame: u64, keep_frames: u64) {
        let map = &mut self.map;
        let arena = &mut self.arena;
        let free_heads = &mut self.free_heads;
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
            let dies_at = slot.get().last_use + keep_frames + 1;
            if dies_at > current_frame {
                probe.refiles.bump();
                return Some(dies_at);
            }
            probe.expiries.bump();
            release(arena, free_heads, slot.remove().span);
            None
        });
    }

    /// Settle the glyphs [`TextEncoder::encode_run`] accumulated in
    /// `pending`: publish them as `key`'s template when the encode was
    /// `complete`, else drop them.
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
        // the allocator writes the disjoint fields — one hash for the
        // whole operation instead of a probe to read the old span and a
        // second to write the new row.
        let Self {
            map,
            arena,
            free_heads,
            pending,
            expiry,
            counters,
        } = self;
        let len = pending.len() as u32;
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
                release(arena, free_heads, row.get().span);
                let span = store(arena, free_heads, counters, pending, len);
                row.insert(EncodedEntry {
                    span,
                    last_use: frame,
                });
            }
            // A new row owes the wheel its first ticket, and this arm is
            // the only place one is filed — which is what makes "one
            // ticket per row, not one per encode" structural.
            Entry::Vacant(slot) => {
                let span = store(arena, free_heads, counters, pending, len);
                slot.insert(EncodedEntry {
                    span,
                    last_use: frame,
                });
                expiry.schedule(key, frame + ENCODED_CACHE_KEEP_FRAMES + 1);
            }
        }
        pending.clear();
    }
}

/// Reserve a block for a row of `len` glyphs and copy `pending` into it,
/// answering the row's span.
///
/// A glyphless run — all-whitespace text, or every glyph skipped as
/// imageless — is a legitimate complete encode. It owns no block, so it
/// must not reach the allocator, and the empty span it stores makes its
/// later [`release`] a no-op.
///
/// A free function over the fields rather than a method, for the same
/// reason [`release`] is: [`EncodedCache::settle`] calls it while
/// holding a `map` entry, so only a borrow of the disjoint fields stays
/// legal.
fn store(
    arena: &mut Vec<EncodedGlyph>,
    free_heads: &mut Vec<u32>,
    probe: &mut EncodedCounters,
    pending: &[EncodedGlyph],
    len: u32,
) -> Span {
    if len == 0 {
        return Span::new(0, 0);
    }
    let start = alloc_block(arena, free_heads, probe, len);
    arena[start as usize..start as usize + len as usize].copy_from_slice(pending);
    Span::new(start, len)
}

/// Reserve a block for a row of `len` glyphs, reusing a freed block of
/// the same size class when one is available and extending the arena
/// when it is not.
///
/// The extension is the only path that grows the arena, so
/// [`EncodedCounters::block_allocs`] going quiet is exactly the statement
/// "the working set is saturated and every row now recycles".
fn alloc_block(
    arena: &mut Vec<EncodedGlyph>,
    free_heads: &mut Vec<u32>,
    probe: &mut EncodedCounters,
    len: u32,
) -> u32 {
    let class = block_class(len);
    if free_heads.len() <= class {
        free_heads.resize(class + 1, NIL);
    }
    let head = free_heads[class];
    if head != NIL {
        free_heads[class] = arena[head as usize].next_free();
        probe.block_reuses.bump();
        return head;
    }
    probe.block_allocs.bump();
    let start = arena.len() as u32;
    arena.resize(
        start as usize + block_capacity(class) as usize,
        EncodedGlyph::free_link(NIL),
    );
    start
}

/// Hand `span`'s block back to its size class.
///
/// A free function taking the one field it needs, so it can be called
/// from inside [`EncodedCache::sweep`]'s drain closure, which already
/// holds `map` and `probe` borrowed.
///
/// The class is recovered from `span.len` rather than stored: every
/// block was allocated by [`EncodedCache::alloc_block`] for exactly this
/// length, so `block_class` maps it back to the list it came from. An
/// empty span owns no block.
pub(super) fn release(arena: &mut [EncodedGlyph], free_heads: &mut [u32], span: Span) {
    if span.len == 0 {
        return;
    }
    let class = block_class(span.len);
    debug_assert!(
        class < free_heads.len(),
        "a live row's size class must already exist — it was allocated from",
    );
    // Push onto the class's chain: the block's first slot takes the old
    // head, and the block itself becomes the new one.
    arena[span.start as usize] = EncodedGlyph::free_link(free_heads[class]);
    free_heads[class] = span.start;
}

/// Frames an unused [`EncodedCache`] entry survives before being swept
/// in [`TextEncoder::end_frame`]. Keeps the cache from growing
/// unboundedly under a long zoom gesture while comfortably outliving
/// any short flicker (visibility toggle, hover paint) that drops a run
/// for a frame.
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
/// Making the two *equal* would cost population for nothing.
/// `EncodedKey` folds `scale_q` and (through [`TextShapeKey`])
/// `max_w_q`, so a zoom or width drag mints a fresh key per run per
/// frame that will never be asked for again — and with one window and
/// no demotion signal each of those lives the full span. The resident
/// population is `runs × (KEEP + 1)`, so the window *is* the population
/// multiplier: 120 held 121 frames of dead gesture keys, ~27 MB of
/// glyph templates for a text-dense drag, on an arena that never
/// shrinks.
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
    ENCODED_CACHE_KEEP_FRAMES <= crate::text::RENDERED_RUN_KEEP_FRAMES,
    "the shaped-buffer window must cover the encoded-run window",
);
