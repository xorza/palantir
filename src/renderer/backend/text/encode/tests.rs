use super::*;
use crate::renderer::backend::text::ContentType;
use crate::renderer::backend::text::encode::cache::NIL;
use crate::renderer::backend::text::encode::encoder::pack_uv;

fn key(scale_q: u32) -> EncodedKey {
    EncodedKey {
        text: TextShapeKey::INVALID,
        scale_q,
        area_color: 0,
        bins: 0,
    }
}

/// Distinguishable glyph payload — `tag` reaches every field, so a
/// block handed to the wrong row, or one read past the glyphs it
/// actually holds, can't pass.
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
    cache.pending.extend(tags.map(glyph));
    cache.settle(k, at, true);
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

/// The property that replaced compaction: a re-encoded row hands its
/// old block straight back, and the next row of that size takes it,
/// so the arena stops growing once every size class has been seen.
///
/// Hand-traced with a 10-glyph untouched row plus a 4-glyph run
/// re-encoded every frame. `BLOCK_GRANULE` is 4, so the 10-glyph row
/// takes a 12-slot block and the 4-glyph run a 4-slot one: the arena
/// reaches 16 slots on frame 1 and **never grows again**, because
/// every later re-encode of the 4-glyph run frees a 4-slot block and
/// immediately reclaims it.
///
/// The old append-only arena grew 14, 18, 22, 26 and then copied all
/// 14 live glyphs on frame 5 to get back to 14. That copy is what
/// this removes — and removes rather than spreads, so there is no
/// frame anywhere in the sequence that pays more than any other.
#[test]
fn a_reencoded_row_reclaims_its_own_block_and_the_arena_stops_growing() {
    let mut cache = EncodedCache::default();
    // Untouched row: 10 glyphs, well inside its keep window.
    insert(&mut cache, key(1), 1000..1010, 0);
    assert_eq!(
        cache.arena.len(),
        12,
        "10 glyphs round up to a 12-slot block"
    );

    let before = cache.counters.counts();
    for frame in 1u64..=8 {
        let base = frame as u32 * 10;
        insert(&mut cache, key(2), base..base + 4, frame);
        cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
        assert_eq!(
            cache.arena.len(),
            16,
            "after frame {frame}: the re-encode must reuse its own block",
        );
    }
    let delta = cache.counters.counts() - before;
    assert_eq!(
        (delta.block_allocs, delta.block_reuses),
        (1, 7),
        "only the first re-encode extends the arena; the rest recycle",
    );
    assert_eq!(cache.map.len(), 2, "neither row is past its keep window");

    // Blocks never move, so the untouched row's span is still the one
    // it was given and still holds its own glyphs byte-for-byte.
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
        for (got, want) in cache.arena[span.range()].iter().zip(tags.map(glyph)) {
            assert!(same(got, &want), "a live block was disturbed: {got:?}");
        }
    }
}

/// Recycling is per size class, and a block is only ever handed to a
/// row that fits it. Three lengths spanning three classes, freed and
/// re-taken in a different order than they were allocated.
#[test]
fn blocks_recycle_only_within_their_size_class() {
    let mut cache = EncodedCache::default();
    // 2 → class 0 (4 slots), 5 → class 1 (8), 9 → class 2 (12).
    for (i, len) in [2u32, 5, 9].into_iter().enumerate() {
        insert(&mut cache, key(i as u32), 0..len, 0);
    }
    assert_eq!(cache.arena.len(), 4 + 8 + 12);
    let spans: Vec<Span> = (0..3).map(|i| cache.map[&key(i)].span).collect();

    // Expire all three.
    for frame in 1..=ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
    }
    assert!(cache.map.is_empty());

    // Re-insert in the reverse order: each must land back in the
    // block of its own class, so the arena does not grow at all.
    let before = cache.counters.counts();
    for (i, len) in [9u32, 5, 2].into_iter().enumerate() {
        insert(
            &mut cache,
            key(100 + i as u32),
            0..len,
            ENCODED_CACHE_KEEP_FRAMES + 1,
        );
    }
    assert_eq!(
        cache.arena.len(),
        4 + 8 + 12,
        "no class needed a fresh block"
    );
    let delta = cache.counters.counts() - before;
    assert_eq!((delta.block_allocs, delta.block_reuses), (0, 3));
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

/// A row whose length lands mid-class shares a block with any other
/// length in that class, and the slack past `span.len` belongs to
/// nobody — so a shorter row reusing a longer row's block must not
/// read the tail it did not write.
#[test]
fn a_shorter_row_reusing_a_block_exposes_only_its_own_glyphs() {
    let mut cache = EncodedCache::default();
    insert(&mut cache, key(1), 700..704, 0); // 4 glyphs, class 0
    let block = cache.map[&key(1)].span;
    for frame in 1..=ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
    }
    // 1 glyph, same class — takes the same block, writes one slot.
    insert(&mut cache, key(2), 900..901, ENCODED_CACHE_KEEP_FRAMES + 1);
    let span = cache.map[&key(2)].span;
    assert_eq!(span.start, block.start, "same class, recycled block");
    assert_eq!(span.len, 1, "the span covers only what was written");
    assert!(same(&cache.arena[span.range()][0], &glyph(900)));
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
        let arena_before = cache.arena.len();
        cache.pending.extend((200..202).map(glyph));

        cache.settle(key(2), 9, complete);
        assert!(cache.pending.is_empty(), "settle consumes the pending row");

        assert_eq!(
            cache.map.contains_key(&key(2)),
            complete,
            "complete = {complete}",
        );
        assert_eq!(cache.map.len(), 1 + expect_rows, "complete = {complete}");
        assert_eq!(
            cache.arena.len(),
            if complete {
                arena_before + 4
            } else {
                arena_before
            },
            "an incomplete encode must reserve no block",
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
            assert_eq!((span.start, span.len), (arena_before as u32, 2));
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
        cache.arena.len(),
        ROWS as usize * 4,
        "steady redraw allocates one block per row and never another",
    );

    // And they still die once the redraw stops — the re-filing did
    // not push the deadline out of reach.
    let last = ENCODED_CACHE_KEEP_FRAMES * 3;
    for frame in last + 1..=last + ENCODED_CACHE_KEEP_FRAMES + 1 {
        cache.sweep(frame, ENCODED_CACHE_KEEP_FRAMES);
    }
    assert!(cache.map.is_empty(), "rows outlived their window");
    assert_eq!(
        cache.free_heads.iter().filter(|&&h| h != NIL).count(),
        1,
        "every expired row's block went back to its one size class",
    );
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
/// already handles the drain, and the block allocator now handles
/// the storage; what is left is the *population*, which is a
/// retention question rather than a per-frame-cost one.
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

/// **The property the block allocator exists for.** Under a
/// sustained gesture every frame mints `RUNS` rows and expires
/// `RUNS` rows, so once the population saturates the arena has seen
/// every block it will ever need and each frame's work is exactly:
/// `RUNS` blocks off a free list, `RUNS` blocks back onto it. No
/// frame in the steady state does anything another does not.
///
/// This is what replaced the compaction, so it is asserted as an
/// absolute rather than a ratio: `block_allocs == 0` says the arena
/// did not grow by a single slot, and the arena length holding
/// constant says nothing was relocated. The old design could not
/// state either — its arena grew every frame by construction and
/// gave the space back in one 122-frame-periodic copy.
#[test]
fn a_saturated_gesture_reaches_a_steady_state_where_no_frame_allocates() {
    const RUNS: u32 = 8;
    const GLYPHS: u32 = 12;
    let mut churn = internals::ChurnBench::new(RUNS, GLYPHS);

    // Warm past the keep window so every frame both mints and
    // expires a full complement of rows.
    for _ in 0..ENCODED_CACHE_KEEP_FRAMES * 2 {
        churn.churn_frame();
    }
    let saturated_arena = churn.arena_len();
    let before = churn.counts();

    const MEASURED: u64 = ENCODED_CACHE_KEEP_FRAMES;
    for _ in 0..MEASURED {
        churn.churn_frame();
    }

    let delta = churn.counts() - before;
    assert_eq!(
        churn.arena_len(),
        saturated_arena,
        "a saturated gesture must not grow the arena by one slot",
    );
    assert_eq!(
        delta.block_allocs, 0,
        "every row in the steady state must come off a free list",
    );
    assert_eq!(
        delta.block_reuses,
        RUNS * MEASURED as u32,
        "and every row must take exactly one block",
    );
    // The population itself is unchanged — this is a steady state,
    // not a cache that quietly stopped retaining.
    assert_eq!(
        churn.rows(),
        RUNS as usize * (ENCODED_CACHE_KEEP_FRAMES as usize + 1),
    );
    // Sized by the peak *concurrent* block count, which is one frame
    // ahead of the resident row count: a frame encodes its rows
    // before `end_frame` sweeps, so `KEEP + 1` frames of rows are
    // live while frame `KEEP + 2`'s blocks are being taken. 12
    // glyphs is exactly three granules, so beyond that this workload
    // wastes nothing.
    let window = ENCODED_CACHE_KEEP_FRAMES as usize + 1;
    assert_eq!(
        saturated_arena,
        RUNS as usize * (window + 1) * GLYPHS as usize,
        "one frame of headroom over the {window}-frame resident window",
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
