//! The rect grid's overlap queries: empty and zero-area inputs, one tile,
//! tile boundaries, long chains, and a linear scan as the oracle.

use crate::primitives::urect::URect;
use crate::renderer::frontend::composer::rect_grid::{RectGrid, TILE_CAP};
use glam::UVec2;

#[test]
fn rect_grid_empty_returns_no_overlap() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(10, 10, 50, 50)));
}

#[test]
fn rect_grid_zero_area_input_is_ignored() {
    // Push: zero w/h rects don't enter the index (they can't
    // intersect anything anyway). Query: zero w/h queries
    // short-circuit to false.
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 0, 50));
    g.push(URect::new(10, 10, 50, 0));
    assert_eq!(g.rects.len(), 0, "zero-area pushes don't grow the index");
    g.push(URect::new(10, 10, 50, 50));
    assert!(!g.any_overlap(URect::new(10, 10, 0, 50)));
    assert!(!g.any_overlap(URect::new(10, 10, 50, 0)));
}

#[test]
fn rect_grid_finds_within_single_tile() {
    let mut g = RectGrid::default();
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
fn rect_grid_finds_across_tile_boundaries() {
    // Tile size is 64. A rect spanning tile boundary registers into
    // multiple tiles; queries from either tile must hit.
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(60, 60, 20, 20));
    assert!(g.any_overlap(URect::new(60, 60, 4, 4)), "left tile hit");
    assert!(g.any_overlap(URect::new(76, 76, 4, 4)), "right tile hit");
    assert!(g.any_overlap(URect::new(64, 64, 1, 1)), "boundary tile hit");
}

/// A tile's rects past its inline row chain off it, and so does every
/// rect whose index does not fit the row's `u16`. 65 537 rects in one
/// tile: 8 inline, 65 529 chained, the last of them index 65 536.
#[test]
fn rect_grid_chains_past_the_row_and_the_u16_index() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(64, 64));
    let indexed = URect::new(0, 0, 1, 1);
    for _ in 0..u16::MAX as usize + 1 {
        g.push(indexed);
    }
    let past_u16 = URect::new(10, 10, 1, 1);
    g.push(past_u16);

    assert_eq!(g.rects.len(), u16::MAX as usize + 2);
    assert_eq!(g.lens[0] as usize, TILE_CAP);
    assert_eq!(g.overflow.len(), u16::MAX as usize + 2 - TILE_CAP);
    assert!(g.any_overlap(indexed));
    assert!(g.any_overlap(past_u16));
    assert!(!g.any_overlap(URect::new(20, 20, 1, 1)));
}

/// A full tile chains its overflow to itself alone: a query reaches a
/// chained rect from every tile the rect overlaps, and a query that
/// touches none of them walks no chain.
#[test]
fn rect_grid_chains_are_per_tile_and_clear() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(256, 256));
    // 10 rects all overlapping tiles (0,0) and (1,0): each tile holds
    // TILE_CAP inline and chains the last 2.
    for i in 0..10u32 {
        g.push(URect::new(60, i * 3, 8, 2));
    }
    assert_eq!(g.lens[0] as usize, TILE_CAP);
    assert_eq!(g.lens[1] as usize, TILE_CAP);
    assert_eq!(g.overflow.len(), 4, "two links in each of the two tiles");
    // The 9th rect (y=24..26) is chained in both tiles; a query touching
    // just it must hit from either.
    assert!(g.any_overlap(URect::new(60, 24, 1, 1)));
    assert!(g.any_overlap(URect::new(66, 27, 1, 1)));
    assert!(!g.any_overlap(URect::new(60, 40, 1, 1)));
    g.clear();
    assert!(!g.any_overlap(URect::new(60, 24, 1, 1)));
    assert_eq!(g.overflow.len(), 0);

    // Three neighbouring tiles saturated, then one rect spanning all
    // three: it chains once in each, and is found from each.
    for tx in 0..3u32 {
        for i in 0..TILE_CAP as u32 {
            g.push(URect::new(tx * 64 + 1, i * 3, 8, 2));
        }
    }
    assert_eq!(g.overflow.len(), 0, "each rect fits inside its own tile");
    // y = 30 is clear of the saturating rects (they occupy y 0..23),
    // so a hit here can only come from this rect.
    g.push(URect::new(0, 30, 192, 2));
    assert_eq!(g.overflow.len(), 3, "one link in each saturated tile");
    for tx in 0..3u32 {
        assert!(
            g.any_overlap(URect::new(tx * 64 + 2, 30, 1, 1)),
            "spanning rect must be found from tile {tx}",
        );
    }
    assert!(!g.any_overlap(URect::new(2, 40, 1, 1)));
}

#[test]
fn rect_grid_matches_linear_scan_on_random_workload() {
    // Cross-check: for a synthetic workload, the grid agrees with a
    // flat linear scan across many queries. Catches regressions where
    // the tile-range math (off-by-one on edges, missing the
    // last-pixel tile) lets a query miss a registered rect.
    let mut g = RectGrid::default();
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
fn rect_grid_clear_drops_all_rects() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
    g.clear();
    assert_eq!(g.rects.len(), 0);
    assert!(!g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn rect_grid_shrinks_viewport_without_visible_stale_state() {
    // start_frame is grow-only: a smaller-viewport frame reuses the
    // larger backing vector, but the active grid still answers
    // correctly. The previous frame's rects must NOT show up after
    // start_frame clears.
    let mut g = RectGrid::default();
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
fn rect_grid_start_frame_is_grow_only() {
    // Internal contract: shrinking the viewport doesn't free the
    // tile storage — it stays sized to the high-water mark so the
    // resize-arm benchmark (cycling between viewports) doesn't
    // re-drop and re-allocate tile rows every frame.
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    let big = g.slots.len();
    g.start_frame(UVec2::new(256, 256));
    assert_eq!(g.slots.len(), big, "shrink must not deallocate tiles");
    assert_eq!(g.lens.len(), big, "lens stays parallel to slots");
}
