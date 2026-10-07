//! The encoded-glyph cache: rows aging out past the keep window, and a re-encoded row reclaiming its own block.

use super::*;
use crate::common::counters::CounterSet;
use crate::text::key::TextShapeKey;

/// The scale rung is this fixture's only axis, so one run identity stands for the text across every row.
fn key(scale_q: u32) -> EncodedKey {
    EncodedKey {
        text: TextShapeKey::fixture(),
        scale_q,
        area_color: 0,
        bins: 0,
    }
}

/// Distinguishable glyph payload: `tag` reaches every field, so a misrouted or over-read block can't pass.
fn glyph(tag: u32) -> EncodedGlyph {
    EncodedGlyph {
        instance: RasterQuad {
            pos: [tag as i32, -(tag as i32)],
            dim: [tag as u16, (tag >> 16) as u16],
            size: [(tag ^ 0x5a5a) as u16, 0],
            uv_and_kind: tag << 8,
            color: bytemuck::cast(u64::from(!tag)),
        },
        atlas_slot: tag,
        generation: tag + 1,
    }
}

/// Byte-exact comparison; `RasterQuad` is `Pod`, so any dropped field is caught.
fn same(a: &EncodedGlyph, b: &EncodedGlyph) -> bool {
    bytemuck::bytes_of(&a.instance) == bytemuck::bytes_of(&b.instance)
        && a.atlas_slot == b.atlas_slot
        && a.generation == b.generation
}

/// Push `glyphs` onto the arena and point `k` at them, as a re-encode would.
fn insert(cache: &mut EncodedCache, k: EncodedKey, tags: impl Iterator<Item = u32>, at: u64) {
    for tag in tags {
        cache.stage(glyph(tag));
    }
    cache.settle(k, at, true);
}

/// Sweeping every frame makes retention exact: a row last used on frame `L` dies at `L + KEEP + 1`, whenever it was touched. Two offsets pin that the death frame tracks `L` rather than a grid.
#[test]
fn unused_rows_die_one_frame_past_the_keep_window() {
    for last_use in [0u64, 9] {
        let mut cache = EncodedCache::default();
        insert(&mut cache, key(1), 0..1, last_use);
        let mut died = None;
        for frame in last_use + 1..=last_use + 400 {
            cache.sweep(frame);
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

/// A re-encoded row hands its old block straight back and the next row of that size takes it, so the arena stops growing once every size class has been seen. Hand-traced: a 10-glyph row (12-slot block, `GRANULE` is 4) plus a 4-glyph run re-encoded every frame reach 16 slots on frame 1 and never grow again.
#[test]
fn a_reencoded_row_reclaims_its_own_block_and_the_arena_stops_growing() {
    let mut cache = EncodedCache::default();
    insert(&mut cache, key(1), 1000..1010, 0);
    assert_eq!(
        cache.arena.slots.len(),
        12,
        "10 glyphs round up to a 12-slot block"
    );

    let before = cache.arena.counters.counts();
    for frame in 1u64..=8 {
        let base = frame as u32 * 10;
        insert(&mut cache, key(2), base..base + 4, frame);
        cache.sweep(frame);
        assert_eq!(
            cache.arena.slots.len(),
            16,
            "after frame {frame}: the re-encode must reuse its own block",
        );
    }
    let delta = cache.arena.counters.counts() - before;
    assert_eq!(
        (delta.allocs, delta.reuses),
        (1, 7),
        "only the first re-encode extends the arena; the rest recycle",
    );
    assert_eq!(cache.map.len(), 2, "neither row is past its keep window");

    let untouched = cache.map[&key(1)].span;
    let churned = cache.map[&key(2)].span;
    assert_eq!((untouched.start, untouched.len), (0, 10));
    assert_eq!(churned.len, 4);
    assert!(
        untouched.range().end <= churned.range().start
            || churned.range().end <= untouched.range().start,
        "live blocks must not overlap: {untouched:?} / {churned:?}",
    );
    for (span, tags) in [(untouched, 1000..1010), (churned, 80..84)] {
        for (got, want) in cache.arena.slots[span.range()].iter().zip(tags.map(glyph)) {
            assert!(same(got, &want), "a live block was disturbed: {got:?}");
        }
    }
}

/// Recycling is per size class; a block goes only to a row that fits it. Three lengths across three classes are freed and re-taken in a different order.
#[test]
fn blocks_recycle_only_within_their_size_class() {
    let mut cache = EncodedCache::default();
    for (i, len) in [2u32, 5, 9].into_iter().enumerate() {
        insert(&mut cache, key(i as u32), 0..len, 0);
    }
    assert_eq!(cache.arena.slots.len(), 4 + 8 + 12);
    let spans: Vec<Span> = (0..3).map(|i| cache.map[&key(i)].span).collect();

    // Expire all three.
    for frame in 1..=ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame);
    }
    assert!(cache.map.is_empty());

    let before = cache.arena.counters.counts();
    for (i, len) in [9u32, 5, 2].into_iter().enumerate() {
        insert(
            &mut cache,
            key(100 + i as u32),
            0..len,
            ENCODED_CACHE_KEEP_FRAMES + 1,
        );
    }
    assert_eq!(
        cache.arena.slots.len(),
        4 + 8 + 12,
        "no class needed a fresh block"
    );
    let delta = cache.arena.counters.counts() - before;
    assert_eq!((delta.allocs, delta.reuses), (0, 3));
    assert_eq!(
        cache.map[&key(100)].span.start,
        spans[2].start,
        "9 → the 12-slot block"
    );
    assert_eq!(
        cache.map[&key(101)].span.start,
        spans[1].start,
        "5 → the 8-slot block"
    );
    assert_eq!(
        cache.map[&key(102)].span.start,
        spans[0].start,
        "2 → the 4-slot block"
    );
}

/// A shorter row reusing a longer row's block must not read the slack past `span.len`, which belongs to nobody.
#[test]
fn a_shorter_row_reusing_a_block_exposes_only_its_own_glyphs() {
    let mut cache = EncodedCache::default();
    insert(&mut cache, key(1), 700..704, 0); // 4 glyphs, class 0
    let block = cache.map[&key(1)].span;
    for frame in 1..=ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame);
    }
    insert(&mut cache, key(2), 900..901, ENCODED_CACHE_KEEP_FRAMES + 1);
    let span = cache.map[&key(2)].span;
    assert_eq!(span.start, block.start, "same class, recycled block");
    assert_eq!(span.len, 1, "the span covers only what was written");
    assert!(same(&cache.arena.slots[span.range()][0], &glyph(900)));
}

/// An incomplete encode (y-culled line, atlas full; both settle as `complete: false`) leaves no map row and no dead glyphs. Caching a short run would replay its hole forever, since the key records neither bounds nor atlas occupancy.
#[test]
fn only_complete_encodes_become_templates() {
    for (complete, expect_rows) in [(true, 1), (false, 0)] {
        let mut cache = EncodedCache::default();
        insert(&mut cache, key(1), 100..103, 7);
        let arena_before = cache.arena.slots.len();
        for tag in 200..202 {
            cache.stage(glyph(tag));
        }

        cache.settle(key(2), 9, complete);
        assert!(cache.pending.is_empty(), "settle consumes the pending row");

        assert_eq!(
            cache.map.contains_key(&key(2)),
            complete,
            "complete = {complete}",
        );
        assert_eq!(cache.map.len(), 1 + expect_rows, "complete = {complete}");
        assert_eq!(
            cache.arena.slots.len(),
            if complete {
                arena_before + 4
            } else {
                arena_before
            },
            "an incomplete encode must reserve no block",
        );
        let survivor = cache.map[&key(1)].span;
        for (got, want) in cache.arena.slots[survivor.range()]
            .iter()
            .zip((100..103).map(glyph))
        {
            assert!(same(got, &want), "settle disturbed a live row: {got:?}");
        }
        if complete {
            let span = cache.map[&key(2)].span;
            assert_eq!((span.start, span.len), (arena_before as u32, 2));
            assert_eq!(cache.map[&key(2)].last_use, 9);
        }
    }
}

/// A sweep costs what expires, not what is resident. A steadily-drawn row files nothing; its one ticket fires once a window, finds it live, and re-files. Filing on every touch would hold `rows × KEEP` tickets.
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
        cache.sweep(frame);
    }

    assert_eq!(cache.map.len(), ROWS as usize, "every row is still live");
    assert_eq!(
        cache.expiry.pending(),
        ROWS as usize,
        "three windows of redraw must not accumulate tickets",
    );
    assert_eq!(
        cache.arena.slots.len(),
        ROWS as usize * 4,
        "steady redraw allocates one block per row and never another",
    );

    let last = ENCODED_CACHE_KEEP_FRAMES * 3;
    for frame in last + 1..=last + ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame);
    }
    assert!(cache.map.is_empty(), "rows outlived their window");
    assert_eq!(
        cache.arena.classes_with_free_blocks(),
        1,
        "every expired row's block went back to its one size class",
    );
}

/// Quantifies the problem a probation tier would solve: a drag asks for each re-keyed run once, yet each lives the full `ENCODED_CACHE_KEEP_FRAMES`, settling the population at `runs × (KEEP + 1)`. A retention question, not a per-frame cost.
#[test]
fn a_gesture_frame_retains_a_full_keep_window_of_single_use_rows() {
    const FRAMES: u64 = ENCODED_CACHE_KEEP_FRAMES * 2;

    const RUNS: u32 = 8;
    const GLYPHS: u32 = 12;
    let mut churn = internals::ChurnBench::new(RUNS, GLYPHS);

    for _ in 0..FRAMES {
        churn.churn_frame();
    }

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

/// **The property the block allocator exists for.** Under a sustained gesture each frame mints and expires `RUNS` rows, so once saturated it takes `RUNS` blocks off the free list and returns `RUNS`. Asserted as absolutes: `allocs == 0` and a constant arena length.
#[test]
fn a_saturated_gesture_reaches_a_steady_state_where_no_frame_allocates() {
    const MEASURED: u64 = ENCODED_CACHE_KEEP_FRAMES;

    const RUNS: u32 = 8;
    const GLYPHS: u32 = 12;
    let mut churn = internals::ChurnBench::new(RUNS, GLYPHS);

    for _ in 0..ENCODED_CACHE_KEEP_FRAMES * 2 {
        churn.churn_frame();
    }
    let saturated_arena = churn.arena_len();
    let before = churn.block_counts();

    for _ in 0..MEASURED {
        churn.churn_frame();
    }

    let delta = churn.block_counts() - before;
    assert_eq!(
        churn.arena_len(),
        saturated_arena,
        "a saturated gesture must not grow the arena by one slot",
    );
    assert_eq!(
        delta.allocs, 0,
        "every row in the steady state must come off a free list",
    );
    assert_eq!(
        delta.reuses,
        RUNS * MEASURED as u32,
        "and every row must take exactly one block",
    );
    assert_eq!(
        churn.rows(),
        RUNS as usize * (ENCODED_CACHE_KEEP_FRAMES as usize + 1),
    );
    // Sized by peak concurrent blocks, one frame ahead of resident rows: a frame encodes before `end_frame` sweeps. 12 glyphs is three granules, so nothing is wasted.
    let window = ENCODED_CACHE_KEEP_FRAMES as usize + 1;
    assert_eq!(
        saturated_arena,
        RUNS as usize * (window + 1) * GLYPHS as usize,
        "one frame of headroom over the {window}-frame resident window",
    );
}

/// The arena is bounded not by the working set but by the sum over size classes of each one's peak concurrent block count: a block returns only to a row of its own class, so run lengths drifting upward strand every class left behind.
///
/// Traced with one fresh key per frame carrying `16 × frame` glyphs (a long line being typed): each frame lands in a class of its own, never revisited. The arena is `8·F·(F + 1)`, quadratic in the longest run, while live slots are linear. So keep the longest run ever encoded small; wrapped text bounds it, and at 12 glyphs a row there is one class and nothing strands.
#[test]
fn drifting_run_lengths_strand_a_block_in_every_class_they_leave() {
    let mut cache = EncodedCache::default();
    let mut arena_at = Vec::new();
    for frame in 1u64..=100 {
        let len = 16 * frame as u32;
        insert(&mut cache, key(len), 0..len, frame);
        cache.sweep(frame);
        if frame == 40 || frame == 100 {
            arena_at.push(cache.arena.slots.len());
        }
    }
    assert_eq!(
        arena_at,
        vec![8 * 40 * 41, 8 * 100 * 101],
        "one block per frame, of 16·frame slots, never reclaimed",
    );

    // Live 42 160 slots against an arena of 80 800 (1.9x) after a hundred frames, and the ratio climbs.
    let window = ENCODED_CACHE_KEEP_FRAMES + 1;
    assert_eq!(cache.map.len(), window as usize);
    let live: usize = cache.map.values().map(|e| e.span.len as usize).sum();
    assert_eq!(live, 16 * (70..=100).sum::<usize>());
    assert_eq!((live, cache.arena.slots.len()), (42_160, 80_800));

    // Bounded: a length returning to a class it left takes the parked block (frame 5's, free since frame 36).
    let before = cache.arena.counters.counts();
    for frame in 101u64..=110 {
        insert(&mut cache, key(9999), 0..80, frame);
        cache.sweep(frame);
    }
    assert_eq!(
        cache.arena.slots.len(),
        80_800,
        "a returning length reuses what its class stranded",
    );
    let delta = cache.arena.counters.counts() - before;
    assert_eq!(
        (delta.allocs, delta.reuses),
        (0, 10),
        "the first row takes frame 5's parked block and the rest take their own",
    );
}
