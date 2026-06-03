//! Spatial index over text-rect AABBs, used by the composer's
//! paint-order overlap checks. Replaces a flat `Vec<URect>` linear scan
//! that dominated compose time in text-dense UIs.

use crate::primitives::urect::URect;
use glam::UVec2;
use tinyvec::TinyVec;

/// Physical-pixel size of one tile in [`TextRectGrid`]. Each text rect
/// is registered into every tile it overlaps; each overlap query walks
/// the tiles a quad covers and intersects against per-tile rect lists.
/// 64 px balances tile count (~4500 for a 4K viewport, fits in L1)
/// against per-tile rect count (typically 1-3 in dense UIs).
const TILE_SIZE: u32 = 64;

/// Per-tile inline capacity for the grid's index lists. Sized
/// empirically from the `frame/resizing` workload (dense UI at 32×
/// bench scale, viewport 3840×4800 phys px): observed max occupancy
/// was **3**. `N = 8` keeps every tile fully inline with substantial
/// headroom in any realistic UI — a 64-px tile holds 2-3 stacked
/// labels in a typical column.
///
/// `TinyVec` rather than `ArrayVec` to keep pathological text-dense
/// workloads (e.g. spreadsheet-grid layouts with tiny fonts and no
/// padding) functional rather than panicking. Once a tile spills to
/// the heap, its `clear()` between batches only resets `len`; the
/// heap buffer is retained, so a one-time allocation amortizes across
/// every subsequent frame. Steady-state alloc-free after warmup
/// holds.
type TileBucket = TinyVec<[u16; 8]>;

/// Spatial index over the open batch's text-rect AABBs. Replaces a
/// flat `Vec<URect>` linear scan that dominated compose time in
/// text-dense UIs. Backed by a row-major grid of tiles
/// ([`TILE_SIZE`] phys px); each rect lives in the tiles it covers,
/// each query walks only the tiles its rect overlaps and may visit a
/// rect twice for rects spanning >1 tile — fine, we early-exit on
/// first hit so duplicate visits cost only constant-factor false
/// positives.
#[derive(Default)]
pub(crate) struct TextRectGrid {
    cols: u32,
    rows: u32,
    /// Per-tile rect-index lists. Row-major: `tiles[ty * cols + tx]`.
    /// The outer `Vec` is retained across batches; each inner
    /// `TinyVec` is cleared (cheap, no dealloc) on [`Self::clear`].
    tiles: Vec<TileBucket>,
    /// Indices (into `tiles`) that received at least one `push` this
    /// frame — the set we walk on [`Self::clear`] instead of the full
    /// row-major grid. A tile is recorded the first time it
    /// transitions from empty to non-empty within a frame; subsequent
    /// pushes to the same tile skip the record. Capacity is retained
    /// across frames.
    ///
    /// Profiling motivation: `Composer::compose` was spending ~37% of
    /// its self-time clearing all ~4500 tiles every frame (4K viewport
    /// / 64-px tiles), even though only ~100-300 actually held
    /// anything in the bench fixture. Tracking touches drops the
    /// per-frame clear walk to the tiles we genuinely touched.
    touched: Vec<u32>,
    /// All rects inserted into the current batch, in insertion order.
    /// `tiles` stores indices into this vec.
    rects: Vec<URect>,
}

impl TextRectGrid {
    /// Reshape to cover `viewport` and reset all state. Called once
    /// per frame at compose start. Cheap when the viewport hasn't
    /// changed (no allocation — the outer `Vec` is already sized).
    pub(crate) fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only — never shrink. A smaller-viewport frame reuses
        // the larger backing vector; tiles beyond the active grid
        // never get touched because `push` clamps indices to
        // `cols - 1` / `rows - 1`. `touched` stores absolute indices
        // into `tiles`, so `clear` works the same regardless of how
        // `cols × rows` map onto positions inside the vec.
        //
        // Profiling motivation: the resize-arm bench cycles through
        // 4 different viewports per frame. With unconditional
        // `tiles.clear()` + `resize_with(...)` the per-frame
        // `drop_in_place` sweep over every old TinyVec dominated
        // `Composer::compose` (~7% of the bench's CPU cycles).
        if want > self.tiles.len() {
            self.tiles.resize_with(want, TileBucket::default);
        }
        self.cols = cols;
        self.rows = rows;
        self.clear();
    }

    /// Drop every registered rect. Only walks the tiles that actually
    /// got pushed to this frame (`touched`), not the full row-major
    /// grid — `~100-300` tile clears in the dense-text fixture vs
    /// `~4500` on the full sweep.
    pub(crate) fn clear(&mut self) {
        for &i in &self.touched {
            self.tiles[i as usize].clear();
        }
        self.touched.clear();
        self.rects.clear();
    }

    /// Register `r`. No-op for zero-area input (degenerate text rects
    /// can't intersect anything anyway).
    pub(crate) fn push(&mut self, r: URect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        // Tile buckets store rect indices as `u16`. Past 65 535 text
        // rects in one batch the cast would wrap and the grid would
        // point at the wrong rect — a silent paint-order corruption.
        // Far above any real text-dense batch, but assert rather than
        // truncate (the field comments anticipate spreadsheet grids).
        assert!(
            self.rects.len() < u16::MAX as usize,
            "TextRectGrid batch exceeded {} rects — u16 index would wrap",
            u16::MAX,
        );
        let idx = self.rects.len() as u16;
        self.rects.push(r);
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (r.x / TILE_SIZE).min(max_x);
        let cy0 = (r.y / TILE_SIZE).min(max_y);
        let cx1 = ((r.x + r.w - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((r.y + r.h - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let tile_idx = (row + tx) as usize;
                let tile = &mut self.tiles[tile_idx];
                // First touch this frame? Track for the next `clear`
                // so we don't have to walk the whole grid.
                let was_empty = tile.is_empty();
                tile.push(idx);
                if was_empty {
                    self.touched.push(tile_idx as u32);
                }
            }
        }
    }

    /// `true` if any registered rect intersects `q`. Returns on first
    /// hit. Walks every tile in `q`'s tile range and checks each
    /// tile's rect list — typical workload visits 1-4 tiles with 1-3
    /// rects each (avg total: ~4-8 intersect tests vs ~120 for the
    /// old flat scan).
    pub(crate) fn any_overlap(&self, q: URect) -> bool {
        if q.w == 0 || q.h == 0 || self.rects.is_empty() {
            return false;
        }
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (q.x / TILE_SIZE).min(max_x);
        let cy0 = (q.y / TILE_SIZE).min(max_y);
        let cx1 = ((q.x + q.w - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((q.y + q.h - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                for &i in self.tiles[(row + tx) as usize].iter() {
                    if self.rects[i as usize].intersect(q).is_some() {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::urect::URect;
    use crate::renderer::frontend::composer::text_grid::TextRectGrid;
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
                let linear = rects.iter().any(|r| r.intersect(q).is_some());
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
        // Internal contract: shrinking the viewport doesn't free the tile
        // vector — it stays sized to the high-water mark so the
        // resize-arm benchmark (cycling between viewports) doesn't
        // re-drop and re-allocate per-tile TinyVecs every frame.
        let mut g = TextRectGrid::default();
        g.start_frame(UVec2::new(2048, 2048));
        let big = g.tiles.len();
        g.start_frame(UVec2::new(256, 256));
        assert_eq!(g.tiles.len(), big, "shrink must not deallocate tiles");
    }
}
