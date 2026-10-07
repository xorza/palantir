//! Overlap queries: empty and zero-area inputs, tile boundaries, long chains, linear-scan oracle.

use crate::primitives::geometry::urect::URect;
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
    // Zero-area rects are not indexed, and zero-area queries return false.
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
    assert!(g.any_overlap(URect::new(20, 15, 5, 5)));
    assert!(!g.any_overlap(URect::new(0, 0, 5, 5)));
    assert!(!g.any_overlap(URect::new(500, 500, 10, 10)));
}

#[test]
fn rect_grid_finds_across_tile_boundaries() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(1024, 1024));
    g.push(URect::new(60, 60, 20, 20));
    assert!(g.any_overlap(URect::new(60, 60, 4, 4)), "left tile hit");
    assert!(g.any_overlap(URect::new(76, 76, 4, 4)), "right tile hit");
    assert!(g.any_overlap(URect::new(64, 64, 1, 1)), "boundary tile hit");
}

/// Rects past a tile's inline row chain off it, as does any index too large for the row's `u16`.
/// 65 537 rects in one tile: 8 inline, the rest chained.
#[test]
fn rect_grid_chains_past_the_row_and_the_u16_index() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(64, 64));
    let indexed = URect::new(0, 0, 1, 1);
    for _ in 0..=(u16::MAX as usize) {
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

/// A full tile chains overflow to itself alone: a chained rect is found from every tile it overlaps.
#[test]
fn rect_grid_chains_are_per_tile_and_clear() {
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(256, 256));
    for i in 0..10u32 {
        g.push(URect::new(60, i * 3, 8, 2));
    }
    assert_eq!(g.lens[0] as usize, TILE_CAP);
    assert_eq!(g.lens[1] as usize, TILE_CAP);
    assert_eq!(g.overflow.len(), 4, "two links in each of the two tiles");
    assert!(g.any_overlap(URect::new(60, 24, 1, 1)));
    assert!(g.any_overlap(URect::new(66, 27, 1, 1)));
    assert!(!g.any_overlap(URect::new(60, 40, 1, 1)));
    g.clear();
    assert!(!g.any_overlap(URect::new(60, 24, 1, 1)));
    assert_eq!(g.overflow.len(), 0);

    for tx in 0..3u32 {
        for i in 0..TILE_CAP as u32 {
            g.push(URect::new(tx * 64 + 1, i * 3, 8, 2));
        }
    }
    assert_eq!(g.overflow.len(), 0, "each rect fits inside its own tile");
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
    let mut g = RectGrid::default();
    let viewport = UVec2::new(800, 600);
    g.start_frame(viewport);
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
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    g.push(URect::new(1500, 1500, 40, 40)); // far outside the smaller viewport
    g.start_frame(UVec2::new(256, 256));
    assert!(!g.any_overlap(URect::new(1500, 1500, 4, 4)));
    g.push(URect::new(10, 10, 40, 40));
    assert!(g.any_overlap(URect::new(20, 20, 5, 5)));
}

#[test]
fn rect_grid_start_frame_is_grow_only() {
    // Shrinking keeps tile storage at the high-water mark so cycling viewports does not reallocate.
    let mut g = RectGrid::default();
    g.start_frame(UVec2::new(2048, 2048));
    let big = g.slots.len();
    g.start_frame(UVec2::new(256, 256));
    assert_eq!(g.slots.len(), big, "shrink must not deallocate tiles");
    assert_eq!(g.lens.len(), big, "lens stays parallel to slots");
}
