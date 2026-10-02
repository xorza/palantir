//! Text-backend tests: GPU regression coverage for the encoded-glyph cache
//! (liveness, clipping) and the atlas empty-entry sweep.
//!
//! The GPU-wire layout pins live with the type they pin, in
//! `raster_atlas::raster_quad` — both passes draw through it, so neither owns it.

use crate::gpu::raster_atlas::test_support::unallocated_dies_at;
use crate::gpu::text::TextBackend;
use crate::gpu::text::tests::text_rig::{PHYSICAL, TextRig};
use crate::primitives::color::rgba_f16::RgbaF16;
use crate::primitives::span::Span;
use crate::primitives::urect::URect;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::text::{RENDERED_RUN_KEEP_FRAMES, RENDERED_RUN_KEEP_SPREAD_MASK};
use glam::Vec2;

mod text_rig;

/// A run that hits the encoded cache must still refresh the LRU
/// `last_use` of every atlas slot it rides. Before the fix the
/// fast path emitted cached uv coords without touching the slots,
/// so a steadily-cached run's slots froze at their rasterization
/// frame and `evict_one` (which fires under zoom's many-sizes
/// atlas pressure) would reclaim a still-live slot and overwrite it
/// with a different glyph — garbled text.
#[test]
fn cached_run_keeps_its_atlas_slots_live() {
    let mut rig = TextRig::new();

    let runs = [rig.row("File", 14.0 * 1.2, Vec2::new(20.0, 20.0))];
    rig.shaper.drop_cosmic_buffers();
    assert!(
        !rig.shaper.has_cosmic_buffer(runs[0].text.key),
        "fixture must start with an evicted shaped buffer",
    );

    // Frame 1: both caches miss, so the backend reconstructs the shaped
    // buffer before rasterizing and caching the encoded glyphs.
    rig.frame(2.0, &[&runs]);
    assert!(
        rig.shaper.has_cosmic_buffer(runs[0].text.key),
        "an encoded-cache miss must restore its shaped buffer",
    );
    let arena_after_warmup = rig.backend.encoder.cache().arena_len();
    rig.backend.tick_frame();
    assert!(
        !rig.backend.pass.atlas.cache.is_empty(),
        "warmup should have rasterized at least one glyph",
    );

    // Frame 2: same run → encoded-cache hit (no cosmic walk, no new
    // rasterization). The hit must still bump every slot's
    // last_use to the now-current frame.
    // A clone shares the shaper's cache, so holding a borrow through it
    // makes any cosmic walk on the hit path panic.
    let shaper = rig.shaper.clone();
    let shaper_borrow = shaper.hold_borrow();
    rig.frame(2.0, &[&runs]);
    drop(shaper_borrow);

    let cf = rig.backend.pass.atlas.current_frame;
    let stale: Vec<u64> = rig
        .backend
        .pass
        .atlas
        .cache
        .values()
        .map(|&i| rig.backend.pass.atlas.slots[i as usize].last_use)
        .filter(|&lu| lu != cf)
        .collect();
    assert!(
        stale.is_empty(),
        "cache-hit frame left slots stale: last_use {stale:?} != current_frame {cf}",
    );
    // The refresh must have gone through the entry's *recorded*
    // slab indices — the exact path the hot loop writes.
    for (_, span) in rig.backend.encoder.cache().resident_rows() {
        for glyph in rig.backend.encoder.cache().templates(span) {
            let idx = glyph.atlas_slot;
            assert_eq!(
                rig.backend.pass.atlas.slots[idx as usize].last_use, cf,
                "recorded slab index {idx} not refreshed on hit",
            );
        }
    }
    assert_eq!(
        rig.backend.encoder.cache().arena_len(),
        arena_after_warmup,
        "a pure cache-hit frame must not append a replacement span",
    );
}

#[test]
fn slot_generation_invalidates_only_referencing_run() {
    let mut rig = TextRig::new();

    let runs = [
        rig.row("AB", 14.0 * 1.2, Vec2::new(20.0, 20.0)),
        rig.row("ZZZZ", 14.0 * 1.2, Vec2::new(20.0, 60.0)),
    ];

    rig.frame(2.0, &[&runs]);
    assert_eq!(rig.backend.encoder.cache().rows(), 2);
    rig.backend.tick_frame();

    let entries: Vec<_> = rig.backend.encoder.cache().resident_rows().collect();
    // Invalidate the two-glyph "AB" run through its *second* glyph:
    // the cache-hit replay then validates and emits "A" before the
    // mismatch, pinning the partial-output rollback rather than a
    // first-glyph bail.
    let (invalidated_key, invalidated_span) = entries
        .iter()
        .copied()
        .find(|(_, span)| span.len == 2)
        .expect("the two-glyph run must have a cached span");
    let invalidated_slot = rig.backend.encoder.cache().templates(invalidated_span)[1].atlas_slot;
    let (stable_key, stable_span) = entries
        .iter()
        .copied()
        .find(|(_, span)| {
            rig.backend
                .encoder
                .cache()
                .templates(*span)
                .iter()
                .all(|glyph| glyph.atlas_slot != invalidated_slot)
        })
        .expect("test runs must use disjoint atlas slots");
    let arena_before = rig.backend.encoder.cache().arena_len();

    let slot = &mut rig.backend.pass.atlas.slots[invalidated_slot as usize];
    slot.generation = slot
        .generation
        .checked_add(1)
        .expect("test slot generation overflowed");
    let expected_generation = slot.generation;
    rig.frame(2.0, &[&runs]);

    assert_eq!(
        rig.backend.pass.instances.len(),
        6,
        "the rolled-back hit must not leak its partially emitted glyphs",
    );
    assert_eq!(
        rig.backend.encoder.cache().span_of(&stable_key),
        Some(stable_span),
        "a disjoint run must retain its encoded span",
    );
    // The rebuild is proven by the generation below, not by where
    // the row landed: dropping the stale row frees its block, and
    // the re-encode is the same length, so it reclaims that very
    // block. Asserting *that* is the stronger statement — an
    // invalidation must not cost arena growth.
    let replacement = rig
        .backend
        .encoder
        .cache()
        .span_of(&invalidated_key)
        .expect("the invalidated run must be re-encoded");
    assert_eq!(
        replacement, invalidated_span,
        "the rebuilt run must reclaim the block its stale template freed",
    );
    assert_eq!(
        rig.backend.encoder.cache().arena_len(),
        arena_before,
        "a slot invalidation must not grow the arena",
    );
    assert_eq!(
        rig.backend.encoder.cache().templates(replacement)[1].generation,
        expected_generation,
        "the replacement must record the slot's new generation",
    );
}

/// Two batches prepared in one frame ride a single deferred vbuf
/// write (`TextBackend::flush` after all `prepare_batch` calls). The
/// per-batch `ranges` must partition the shared instance vec and
/// each batch's glyphs must keep their own color/placement — same
/// text at a different origin/color pins this glyph-by-glyph: same
/// atlas uv + dim, x identical, y shifted by exactly the origin
/// delta (40 px, integer so subpixel bins match), colors distinct.
#[test]
fn deferred_upload_keeps_batches_distinct() {
    let mut rig = TextRig::new();

    let color_a = RgbaF16::new(0.94, 0.94, 0.94, 1.0);
    let color_b = RgbaF16::new(0.78, 0.39, 0.2, 1.0);
    let run_a = TextDrawRow {
        color: color_a,
        ..rig.row("File", 16.8, Vec2::new(20.0, 20.0))
    };
    let run_b = TextDrawRow {
        color: color_b,
        ..rig.row("File", 16.8, Vec2::new(20.0, 60.0))
    };

    rig.frame(
        1.0,
        &[std::slice::from_ref(&run_a), std::slice::from_ref(&run_b)],
    );

    // Same text → same glyph count n per batch; ranges partition
    // the vec as [0..n] + [n..2n].
    let n = rig.backend.pass.instances.len() / 2;
    assert_eq!(n, 4, "'File' shapes to one glyph per character");
    assert_eq!(rig.backend.pass.batch_span(0), Span::new(0, n as u32));
    assert_eq!(
        rig.backend.pass.batch_span(1),
        Span::new(n as u32, n as u32)
    );

    for (ga, gb) in rig.backend.pass.instances[..n]
        .iter()
        .zip(&rig.backend.pass.instances[n..2 * n])
    {
        assert_eq!(ga.color, color_a);
        assert_eq!(gb.color, color_b);
        // Identical glyph, identical atlas slot, shifted 40 px down.
        assert_eq!(gb.uv_and_kind, ga.uv_and_kind);
        assert_eq!(gb.dim, ga.dim);
        assert_eq!(gb.pos, [ga.pos[0], ga.pos[1] + 40]);
    }
    rig.backend.tick_frame();
}

/// A run whose lines are partially y-culled by its bounds must not
/// populate the encoded cache: `EncodedKey` omits bounds, so after
/// integer-pixel scrolling the same key would replay the truncated
/// template and newly revealed lines would stay blank forever.
#[test]
fn partially_culled_run_is_not_cached() {
    let mut rig = TextRig::new();

    // Three 3-glyph lines at line_height 16 px, origin (0, 0):
    // line tops sit at 0 / 16 / 32.
    let mut run = rig.row("abc\ndef\nxyz", 16.0, Vec2::ZERO);
    // Clip to the first line: the pre-cull keeps lines with
    // line_top <= bounds_bot, so h = 10 keeps line 0 (top 0) and
    // drops lines 1-2 (tops 16, 32).
    run.bounds = URect::new(0, 0, PHYSICAL.x, 10);

    // Frame 1: clipped encode → 1 line * 3 glyphs = 3 instances,
    // and no cache entry.
    rig.frame(1.0, &[std::slice::from_ref(&run)]);
    assert_eq!(
        rig.backend.pass.instances.len(),
        3,
        "only line 0's 3 glyphs survive the cull"
    );
    assert_eq!(
        rig.backend.encoder.cache().rows(),
        0,
        "a culled encode must not become a cache template",
    );
    rig.backend.tick_frame();

    // Frame 2, same clipped run: still a miss, re-encodes to the
    // same 3 instances, still nothing cached.
    rig.frame(1.0, &[std::slice::from_ref(&run)]);
    assert_eq!(rig.backend.pass.instances.len(), 3);
    assert_eq!(rig.backend.encoder.cache().rows(), 0);
    rig.backend.tick_frame();

    // Frame 3, unclipped: 3 lines * 3 glyphs = 9 instances, and
    // the full encode is cached (same key as the clipped frames —
    // that's exactly why the clipped ones must not insert).
    run.bounds = URect::new(0, 0, PHYSICAL.x, PHYSICAL.y);
    rig.frame(1.0, &[std::slice::from_ref(&run)]);
    assert_eq!(rig.backend.pass.instances.len(), 9);
    assert_eq!(rig.backend.encoder.cache().rows(), 1);
    let (_, cached) = rig
        .backend
        .encoder
        .cache()
        .resident_rows()
        .next()
        .expect("the run is cached");
    assert_eq!(
        cached.len, 9,
        "the whole run is cached, not a culled prefix"
    );
    // Blocks round up to `BLOCK_GRANULE`, so nine glyphs occupy a
    // twelve-slot block. The row's own length is the invariant here;
    // the arena length is the allocator's business.
    assert_eq!(rig.backend.encoder.cache().arena_len(), 12);
    rig.backend.tick_frame();

    // Frame 4 replays the cached template: same 9 instances with
    // no re-encode (the arena didn't grow).
    rig.frame(1.0, &[std::slice::from_ref(&run)]);
    assert_eq!(rig.backend.pass.instances.len(), 9);
    assert_eq!(rig.backend.encoder.cache().rows(), 1);
    assert_eq!(
        rig.backend.encoder.cache().arena_len(),
        12,
        "a hit must not re-encode"
    );
}

/// Both text caches age on one clock, so a frame that draws no text
/// ages neither — and one that draws text ages both by the same
/// step.
///
/// A counter of this side's own, bumped in `end_frame`, would miss every
/// frame that prepared no text batch: a recorded frame whose damage
/// missed every text run would age the shaped-buffer cache and not this
/// one, and `RENDERED_RUN_KEEP_FRAMES` — one constant precisely so a
/// buffer outlives the encoded entry that would come asking for it —
/// would describe two windows measured in different units. Each suite
/// drives one clock, so only this cross-check can catch it.
#[test]
fn both_caches_age_on_one_clock_including_text_free_frames() {
    let mut rig = TextRig::new();

    let runs = [rig.row("aged", 14.0 * 1.2, Vec2::new(20.0, 20.0))];
    rig.frame(1.0, &[&runs]);
    assert_eq!(rig.backend.encoder.cache().rows(), 1, "the run is cached");
    rig.backend.tick_frame();

    // Text-free frames: `prepare_batch` is never called, so `ranges`
    // stays empty — the shape a counter of this side's own would freeze on.
    // Both clocks must still move, in lockstep.
    for _ in 0..8 {
        let before = rig.shaper.frame();
        rig.backend.tick_frame();
        assert_eq!(
            rig.shaper.frame(),
            before + 1,
            "a text-free frame must still advance the shared clock",
        );
        assert_eq!(
            rig.backend.pass.atlas.current_frame,
            rig.shaper.frame(),
            "the atlas must track the shaper's clock, not its own count",
        );
    }

    // And the encoded entry expires on that same clock: it was last
    // used at frame 0, so it dies one frame past its window, without
    // a single text-bearing frame in between.
    assert_eq!(
        rig.backend.encoder.cache().rows(),
        1,
        "premise: still inside the keep window",
    );
    while rig.shaper.frame() <= RENDERED_RUN_KEEP_FRAMES + RENDERED_RUN_KEEP_SPREAD_MASK {
        rig.backend.tick_frame();
    }
    assert_eq!(
        rig.backend.encoder.cache().rows(),
        0,
        "text-free frames must age the encoded cache out",
    );
    assert!(
        !rig.shaper.has_cosmic_buffer(runs[0].text.key),
        "…and the shaped buffer with it, on the same clock — later, by \
         this run's share of `RENDERED_RUN_KEEP_SPREAD_MASK`, but on that clock",
    );
}

/// A zero-area glyph entry (whitespace) swept by the periodic
/// empty-entry sweep must re-insert cleanly through `insert_unallocated`
/// on next use.
#[test]
fn swept_empty_glyph_reinserts() {
    let mut rig = TextRig::new();

    let runs = [rig.row(" ", 16.0, Vec2::new(2.0, 2.0))];
    let empties = |b: &TextBackend| {
        b.pass
            .atlas
            .cache
            .values()
            .filter(|&&i| b.pass.atlas.slots[i as usize].placement.is_none())
            .count()
    };

    rig.frame(1.0, &[&runs]);
    assert!(
        rig.backend.pass.instances.is_empty(),
        "whitespace prepares a text batch without drawable glyphs",
    );
    assert_eq!(
        empties(&rig.backend),
        1,
        "the space rasterizes to one zero-area entry"
    );
    let first_frame = rig.backend.pass.atlas.current_frame;
    rig.backend.tick_frame();
    assert_eq!(
        rig.backend.pass.atlas.current_frame,
        first_frame + 1,
        "a prepared zero-instance batch must still advance cache aging",
    );

    // The space was rasterized on frame 0, so its ticket falls due at
    // `unallocated_dies_at(0)` — its own deadline, not a shared tick.
    // Advance one frame past it with prepared text frames that never
    // touch the space again.
    while rig.backend.pass.atlas.current_frame < unallocated_dies_at(0) + 1 {
        rig.frame(1.0, &[&[]]);
        rig.backend.tick_frame();
    }
    assert_eq!(
        empties(&rig.backend),
        0,
        "stale empty entry reclaimed once its own window lapsed",
    );

    // Re-encoding the same run re-inserts the empty entry (the encoded
    // cache was itself swept after `ENCODED_CACHE_KEEP_FRAMES` idle
    // frames, so this is a full walk through rasterize_and_insert →
    // insert_unallocated).
    rig.frame(1.0, &[&runs]);
    assert_eq!(
        empties(&rig.backend),
        1,
        "swept empty glyph re-inserts on next use"
    );
    rig.backend.tick_frame();
}
