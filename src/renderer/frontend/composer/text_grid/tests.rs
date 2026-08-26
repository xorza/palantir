use crate::primitives::urect::URect;
use crate::renderer::frontend::composer::text_grid::{TILE_CAP, TILE_INDEX_CAPACITY, TextRectGrid};
use glam::UVec2;

#[test]
fn text_grid_empty_returns_no_overlap() {
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(10, 10, 50, 50)));
}

#[test]
fn text_grid_zero_area_input_is_ignored() {
    // Push: zero w/h rects don't enter the index (they can't
    // intersect anything anyway). Query: zero w/h queries
    // short-circuit to false.
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 0, 50));
    g.push(URect::new(10, 10, 50, 0));
    assert_eq!(g.rects.len(), 0, "zero-area pushes don't grow the index");
    g.push(URect::new(10, 10, 50, 50));
    assert!(!g.any_overlap(URect::new(10, 10, 0, 50)));
    assert!(!g.any_overlap(URect::new(10, 10, 50, 0)));
}

#[test]
fn text_grid_finds_within_single_tile() {
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 40, 20));
    // Hit: overlapping rect inside the same tile.
    assert!(g.any_overlap(URect::new(20, 15, 5, 5)));
    // Miss: disjoint rect inside the same tile.
    assert!(!g.any_overlap(URect::new(0, 0, 5, 5)));
    // Miss: disjoint rect in a different tile (far away).
    assert!(!g.any_overlap(URect::new(500, 500, 10, 10)));
}

#[test]
fn text_grid_finds_across_tile_boundaries() {
    // Tile size is 64. A rect spanning tile boundary registers into
    // multiple tiles; queries from either tile must hit.
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(60, 60, 20, 20));
    assert!(g.any_overlap(URect::new(60, 60, 4, 4)), "left tile hit");
    assert!(g.any_overlap(URect::new(76, 76, 4, 4)), "right tile hit");
    assert!(g.any_overlap(URect::new(64, 64, 1, 1)), "boundary tile hit");
}

#[test]
fn text_grid_falls_back_after_tile_index_capacity() {
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(64, 64));
    let indexed = URect::new(0, 0, 1, 1);
    for _ in 0..TILE_INDEX_CAPACITY {
        g.push(indexed);
    }
    let overflow = URect::new(10, 10, 1, 1);
    g.push(overflow);

    // First TILE_CAP pushes land inline; the rest of the u16 index
    // space diverts to `spill`; the final rect exceeds the u16
    // space entirely and only the linear tail sees it.
    assert_eq!(g.rects.len(), TILE_INDEX_CAPACITY + 1);
    assert_eq!(g.lens[0] as usize, TILE_CAP);
    assert_eq!(g.spill.len(), TILE_INDEX_CAPACITY - TILE_CAP);
    assert!(g.any_overlap(indexed));
    assert!(g.any_overlap(overflow));
    assert!(!g.any_overlap(URect::new(20, 20, 1, 1)));
}

#[test]
fn text_grid_spill_hits_from_other_tiles_and_clears() {
    // Fill one tile past TILE_CAP with rects that also span a
    // second tile — the spilled copies must still be findable from
    // any query position (spill is scanned tile-blind), and clear()
    // must drop them.
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(256, 256));
    // 10 rects all overlapping tiles (0,0) and (1,0): tile 0 holds
    // TILE_CAP inline, tile 1 the same, and the last 2 fit neither.
    for i in 0..10u32 {
        g.push(URect::new(60, i * 3, 8, 2));
    }
    assert_eq!(g.lens[0] as usize, TILE_CAP);
    assert_eq!(
        g.spill.len(),
        2,
        "one entry per spilled rect, not one per full tile it spans",
    );
    // The 9th rect (y=24..26) exists only in spill for both tiles;
    // a query touching just it must still hit.
    assert!(g.any_overlap(URect::new(60, 24, 1, 1)));
    assert!(g.any_overlap(URect::new(66, 27, 1, 1)));
    assert!(!g.any_overlap(URect::new(60, 40, 1, 1)));
    g.clear();
    assert!(!g.any_overlap(URect::new(60, 24, 1, 1)));
    assert_eq!(g.spill.len(), 0);

    // Widen the axis: three neighbouring tiles saturated, then one
    // rect spanning all three. It is unfindable by tile — every one
    // is full — so the single spill entry has to carry it, and a
    // per-tile push would have queued three copies for every future
    // query to re-test.
    for tx in 0..3u32 {
        for i in 0..TILE_CAP as u32 {
            g.push(URect::new(tx * 64 + 1, i * 3, 8, 2));
        }
    }
    assert_eq!(g.spill.len(), 0, "each rect fits inside its own tile");
    // y = 30 is clear of the saturating rects (they occupy y 0..23),
    // so a hit here can only come from this rect.
    g.push(URect::new(0, 30, 192, 2));
    assert_eq!(g.spill.len(), 1, "one entry for three saturated tiles");
    for tx in 0..3u32 {
        assert!(
            g.any_overlap(URect::new(tx * 64 + 2, 30, 1, 1)),
            "spanning rect must be found from tile {tx}",
        );
    }
    assert!(!g.any_overlap(URect::new(2, 40, 1, 1)));
}

#[test]
fn text_grid_matches_linear_scan_on_random_workload() {
    // Cross-check: for a synthetic workload, the grid agrees with a
    // flat linear scan across many queries. Catches regressions where
    // the tile-range math (off-by-one on edges, missing the
    // last-pixel tile) lets a query miss a registered rect.
    let mut g = TextRectGrid::default();
    let viewport = UVec2::new(800, 600);
    g.start_frame(viewport);
    // Tiles of 64 px in an 800x600 viewport — boundaries at
    // 0,64,128,…,768 → 13 cols × 10 rows = 130 tiles.
    let rects = [
        URect::new(0, 0, 10, 10),
        URect::new(60, 60, 20, 20), // spans 2x2 tiles
        URect::new(100, 100, 50, 50),
        URect::new(250, 80, 80, 40),
        URect::new(500, 400, 100, 100),
        URect::new(0, 500, 800, 30), // full-width strip
        URect::new(640, 0, 40, 600), // full-height strip
    ];
    for r in rects {
        g.push(r);
    }
    // Probe a grid of query rects and confirm grid ↔ linear scan
    // verdicts agree everywhere.
    for qy in (0..600).step_by(37) {
        for qx in (0..800).step_by(43) {
            let q = URect::new(qx, qy, 20, 20);
            let linear = rects.iter().any(|r| r.intersects(q));
            let grid = g.any_overlap(q);
            assert_eq!(linear, grid, "disagreement at q={q:?}");
        }
    }
}

#[test]
fn text_grid_clear_drops_all_rects() {
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
    g.clear();
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn text_grid_shrinks_viewport_without_visible_stale_state() {
    // start_frame is grow-only: a smaller-viewport frame reuses the
    // larger backing vector, but the active grid still answers
    // correctly. The previous frame's rects must NOT show up after
    // start_frame clears.
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    g.push(URect::new(1500, 1500, 40, 40)); // far outside the smaller viewport
    g.start_frame(UVec2::new(256, 256));
    // Stale rect from the 2048-viewport frame must be cleared even
    // though its physical tile index lives past the new grid.
    assert!(!g.any_overlap(URect::new(1500, 1500, 4, 4)));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn text_grid_start_frame_is_grow_only() {
    // Internal contract: shrinking the viewport doesn't free the
    // tile storage — it stays sized to the high-water mark so the
    // resize-arm benchmark (cycling between viewports) doesn't
    // re-drop and re-allocate tile rows every frame.
    let mut g = TextRectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    let big = g.slots.len();
    g.start_frame(UVec2::new(256, 256));
    assert_eq!(g.slots.len(), big, "shrink must not deallocate tiles");
    assert_eq!(g.lens.len(), big, "lens stays parallel to slots");
}
