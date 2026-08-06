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
use crate::renderer::backend::text::encoded_counters::EncodedCounters;
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

/// End of a size class's free list. Distinguishable from every real
/// block start: a start is an index into the arena, which is bounded by
/// the glyph population, and `u32::MAX` slots of `EncodedGlyph` is 112 GB.
const NIL: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub(super) struct EncodedGlyph {
    instance: GlyphInstance,
    pub(super) atlas_slot: u32,
    pub(super) generation: u32,
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
pub(super) struct EncodedCache {
    pub(super) map: FxHashMap<EncodedKey, EncodedEntry>,
    /// Block storage. Grows to the working set's high-water mark and is
    /// then reused in place; never compacted, so a live row's `span` is
    /// stable for the row's whole life.
    pub(super) arena: Vec<EncodedGlyph>,
    /// Head of each size class's intrusive free list, `NIL` when the
    /// class is empty — `free_heads[c]` starts a chain of blocks of
    /// `block_capacity(c)` slots, linked through their first slot.
    /// LIFO, so the block handed out is the one most recently freed and
    /// therefore the one most likely still in cache.
    ///
    /// Flat by construction: one `u32` per class, and the chain itself
    /// costs nothing because it lives in space that is already free.
    free_heads: Vec<u32>,
    /// Where [`TextEncoder::encode_run`] accumulates a row's glyphs
    /// before its final length is known. [`Self::settle`] either copies
    /// it into a block or drops it, so an incomplete encode costs
    /// nothing but the clear.
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
    /// This side needs it more than that one: [`TextEncoder::
    /// try_emit_cached`] refreshes `last_use` on *every* hit of *every*
    /// visible run, so the previous `map.retain` walked the whole table
    /// every frame purely to discover that nothing had lapsed.
    expiry: ExpiryWheel<EncodedKey>,
    /// Encode / hit / expiry / re-file / block tallies. Zero-sized
    /// outside benchmark and test builds.
    pub(super) counters: EncodedCounters,
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
    fn sweep(&mut self, current_frame: u64, keep_frames: u64) {
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
    fn settle(&mut self, key: EncodedKey, frame: u64, complete: bool) {
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
fn release(arena: &mut [EncodedGlyph], free_heads: &mut [u32], span: Span) {
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
const ENCODED_CACHE_KEEP_FRAMES: u64 = 30;

/// A buffer must outlive the encoded entry that would come asking for
/// it. Stated as an assertion rather than a comment because the two
/// constants now live apart, and the failure it guards is silent —
/// crossing them costs a reshape per miss and nothing reports it.
const _: () = assert!(
    ENCODED_CACHE_KEEP_FRAMES <= crate::text::RENDERED_RUN_KEEP_FRAMES,
    "the shaped-buffer window must cover the encoded-run window",
);

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
    use crate::renderer::backend::text::encoded_counters::EncodedCounts;

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
                    self.cache.pending.push(EncodedGlyph {
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
                self.cache.settle(key, self.frame, true);
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
            self.cache.counters.counts()
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
                for glyph in 0..glyphs_per_row {
                    cache.pending.push(EncodedGlyph {
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
mod tests;
