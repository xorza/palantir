//! Spatial index over paint-rect AABBs, used by the composer's
//! paint-order overlap checks: the text batches' rects, and the
//! higher-kind tiers' once they outgrow a short scan. A flat scan of a
//! `Vec<URect>` costs every query the whole batch, which dominates
//! compose time in text-dense UIs.
//!
//! **The shape of the real workload**, from instrumenting the
//! `frame/*_cpu` arms (25–63 M queries each): ~70–80 rects live at query
//! time (peak 207), 24–34% of queries survive the union pre-reject, and
//! a surviving query walks 7–12 tiles to perform ~0.2–1.6 rect tests.
//! No tile overflows its inline row, and `rects` never approaches the
//! `u16` index space — the overflow chains are what keeps a frame that
//! does bounded by its local density, not tuning knobs.
//!
//! **Why a tiled index and not something simpler**, all measured on
//! `rect_grid/realistic` (200 labels, µs per round):
//!
//! - Union pre-reject + linear scan, i.e. what `higher_kind.rs` does:
//!   **56.7 vs 7.0**. The pre-reject alone does not carry it; at ~75
//!   live rects the surviving third of queries scan far too much. Whole
//!   frames move 1.7% (`scrolling_cpu`) to 3.4% (`cached_cpu`).
//! - A conservative coverage bitmap (one bit per cell, no rect lists, no
//!   spill — legal because a false positive only costs a flush) looks
//!   like a rout at 64-px cells, **4.2 vs 7.0**, until you count
//!   flushes: **35 per compose against 11**, i.e. triple the draw calls.
//!   Shrink cells until behaviour matches (16 px, 11 flushes) and it is
//!   **10% slower than the grid** on `cached_cpu`. Exactness is the
//!   feature being paid for here.
//! - Sorted sweep, interval tree, BVH: all need an O(n log n) build
//!   every frame for n ≈ 200, against a ~7 µs total budget.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::common::counters::TestOnly;
use crate::primitives::urect::URect;
use glam::UVec2;
use std::cell::Cell;

/// Physical-pixel size of one tile in [`RectGrid`]. Each text rect
/// is registered into every tile it overlaps; each overlap query walks
/// the tiles a quad covers and intersects against per-tile rect lists.
///
/// 64 px is a measured optimum, not a guess — `rect_grid/realistic`
/// sweeps it (µs per round, 1920×1080):
///
/// | labels | 32 px | 64 px | 128 px | 256 px |
/// | ------ | ----- | ----- | ------ | ------ |
/// | 64     | 2.53  | 2.20  | 2.23   | 5.16   |
/// | 200    | 8.15  | 7.02  | 7.35   | 36.1   |
/// | 600    | 27.96 | 51.60 | 211.4  | 379.3  |
///
/// Measured when a tile's overflow went to one shared list that every
/// query scanned: past ~200 labels occupancy crossed [`TILE_CAP`], which
/// was the 128/256 px blow-up. Overflow now chains per tile, so a crowded
/// tile costs only the queries that touch it; smaller tiles still cost a
/// steady ~15% in the common case.
pub(super) const TILE_SIZE: u32 = 64;

/// An empty overflow chain.
const NO_OVERFLOW: u32 = u32::MAX;

/// One link of a tile's overflow chain.
#[derive(Clone, Copy, Debug)]
struct OverflowEntry {
    rect: u32,
    next: u32,
}

/// Per-tile inline capacity. Sized empirically from the
/// `frame/resizing` workload (dense UI at 32× bench scale, viewport
/// 3840×4800 phys px): observed max occupancy was **3**. A tile past
/// capacity chains the rest off itself, so pathological text-dense
/// workloads (spreadsheet grids with tiny fonts and no padding) pay a
/// pointer walk in the crowded tiles rather than a scan everywhere.
///
/// The headroom over that observed 3 keeps the realistic workload inline:
/// with a shared overflow list, dropping to `4` bought 3% at 64 labels
/// and cost **4.2×** at 600 (`rect_grid/realistic`: 216.9 µs
/// against 51.6 µs).
pub(super) const TILE_CAP: usize = 8;

/// Spatial index over the open batch's text-rect AABBs. Replaces a
/// flat `Vec<URect>` linear scan that dominated compose time in
/// text-dense UIs. Backed by a row-major grid of tiles
/// ([`TILE_SIZE`] phys px); each rect lives in the tiles it covers,
/// each query walks only the tiles its rect overlaps and may visit a
/// rect twice for rects spanning >1 tile — fine, we early-exit on
/// first hit so duplicate visits cost only constant-factor false
/// positives.
///
/// Tile storage is flat SoA (`lens` + fixed `slots` rows) rather than
/// a `Vec<TinyVec>`: `push`/`any_overlap` are the composer's hottest
/// per-text/per-quad loops, and the inline/heap tag dispatch TinyVec
/// pays on every access profiled at ~2% of the frame on its own.
#[derive(Debug, Default)]
pub(super) struct RectGrid {
    cols: u32,
    rows: u32,
    /// Per-tile occupancy (`0..=TILE_CAP`), row-major
    /// `lens[ty * cols + tx]`. Parallel to `slots`. Reset per batch via
    /// the `touched` walk.
    lens: Vec<u8>,
    /// Per-tile inline rect-index rows; only the first `lens[t]`
    /// entries of `slots[t]` are live.
    slots: Vec<[u16; TILE_CAP]>,
    /// Per-tile head of the overflow chain in [`Self::overflow`], or
    /// [`NO_OVERFLOW`]. Parallel to `lens`, reset by the same `touched`
    /// walk.
    overflow_heads: Vec<u32>,
    /// Every tile's rects past its inline row — the ones pushed after
    /// [`TILE_CAP`], and any whose index does not fit the row's `u16` —
    /// as singly linked chains, newest first. One flat buffer for every
    /// tile, so a crowded tile costs only the queries that walk it: no
    /// rect is ever tested tile-blind.
    overflow: Vec<OverflowEntry>,
    /// Indices (into `lens`/`slots`) that received at least one `push`
    /// this frame — the set we walk on [`Self::clear`] instead of the
    /// full row-major grid. A tile is recorded the first time it
    /// transitions from empty to non-empty within a frame; subsequent
    /// pushes to the same tile skip the record. Capacity is retained
    /// across frames.
    ///
    /// Clearing every tile instead costs ~37% of the compose pass's
    /// self-time at a 4K viewport (~4500 64-px tiles), where the bench
    /// fixture fills only ~100-300.
    touched: Vec<u32>,
    /// All rects inserted into the current batch, in insertion order.
    rects: Vec<URect>,
    /// `intersects` tests the queries ran since the last `start_frame`.
    intersect_tests: TestOnly<Cell<u32>>,
    /// Union AABB of every rect in `rects`. O(1) pre-reject for
    /// [`Self::any_overlap`]: a query outside the union can't hit any
    /// rect, so the tile walk (scattered 32-byte bucket loads from a
    /// grid too big for L1) is skipped entirely. Zero-sized = empty.
    pub(super) union: URect,
}

impl RectGrid {
    /// Reshape to cover `viewport` and reset all state. Called once
    /// per frame at compose start. Cheap when the viewport hasn't
    /// changed (no allocation — the outer `Vec` is already sized).
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only — never shrink. A smaller-viewport frame reuses
        // the larger backing vectors; tiles beyond the active grid
        // never get touched because `push` clamps indices to
        // `cols - 1` / `rows - 1`. `touched` stores absolute indices
        // into `lens`/`slots`, so `clear` works the same regardless of
        // how `cols × rows` map onto positions inside the vecs.
        //
        // A viewport that changes every frame — a resize drag — would
        // otherwise pay a clear and resize sweep over every tile each
        // time.
        if want > self.lens.len() {
            self.lens.resize(want, 0);
            self.slots.resize(want, [0; TILE_CAP]);
            self.overflow_heads.resize(want, NO_OVERFLOW);
        }
        self.cols = cols;
        self.rows = rows;
        self.intersect_tests.reset();
        self.clear();
    }

    /// Drop every registered rect. Only walks the tiles that actually
    /// got pushed to this frame (`touched`), not the full row-major
    /// grid — `~100-300` tile clears in the dense-text fixture vs
    /// `~4500` on the full sweep.
    pub(super) fn clear(&mut self) {
        for &i in &self.touched {
            self.lens[i as usize] = 0;
            self.overflow_heads[i as usize] = NO_OVERFLOW;
        }
        self.touched.clear();
        self.overflow.clear();
        self.rects.clear();
        self.union = URect::ZERO;
    }

    /// Register `r`. No-op for zero-area input (degenerate rects can't
    /// intersect anything anyway).
    pub(super) fn push(&mut self, r: URect) {
        if r.is_paint_empty() {
            return;
        }
        let idx = self.rects.len();
        self.rects.push(r);
        self.union = self.union.union(r);
        let inline = u16::try_from(idx).ok();
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (r.min.x / TILE_SIZE).min(max_x);
        let cy0 = (r.min.y / TILE_SIZE).min(max_y);
        let cx1 = ((r.max().x - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((r.max().y - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let tile = (row + tx) as usize;
                let len = self.lens[tile] as usize;
                let head = self.overflow_heads[tile];
                // First touch this frame? Track for the next `clear` so
                // we don't have to walk the whole grid.
                if len == 0 && head == NO_OVERFLOW {
                    self.touched.push(tile as u32);
                }
                match inline {
                    Some(idx) if len < TILE_CAP => {
                        self.slots[tile][len] = idx;
                        self.lens[tile] = (len + 1) as u8;
                    }
                    _ => {
                        self.overflow_heads[tile] = self.overflow.len() as u32;
                        self.overflow.push(OverflowEntry {
                            rect: idx as u32,
                            next: head,
                        });
                    }
                }
            }
        }
    }

    /// `true` if any registered rect intersects `q`. Returns on first
    /// hit. Walks every tile in `q`'s tile range and checks each
    /// tile's rect list — typical workload visits 1-4 tiles with 1-3
    /// rects each (avg total: ~4-8 intersect tests vs ~120 for the
    /// old flat scan).
    pub(super) fn any_overlap(&self, q: URect) -> bool {
        // The union check subsumes the empty-grid case (an empty grid's
        // zero-sized union intersects nothing).
        if q.is_paint_empty() || !self.union.intersects(q) {
            return false;
        }
        let max_x = self.cols - 1;
        let max_y = self.rows - 1;
        let cx0 = (q.min.x / TILE_SIZE).min(max_x);
        let cy0 = (q.min.y / TILE_SIZE).min(max_y);
        let cx1 = ((q.max().x - 1) / TILE_SIZE).min(max_x);
        let cy1 = ((q.max().y - 1) / TILE_SIZE).min(max_y);
        for ty in cy0..=cy1 {
            let row = ty * self.cols;
            for tx in cx0..=cx1 {
                let t = (row + tx) as usize;
                let n = self.lens[t] as usize;
                for &i in &self.slots[t][..n] {
                    if self.hit(i as u32, q) {
                        return true;
                    }
                }
                let mut link = self.overflow_heads[t];
                while link != NO_OVERFLOW {
                    let entry = self.overflow[link as usize];
                    if self.hit(entry.rect, q) {
                        return true;
                    }
                    link = entry.next;
                }
            }
        }
        false
    }

    fn hit(&self, rect: u32, q: URect) -> bool {
        self.intersect_tests.bump_shared();
        self.rects[rect as usize].intersects(q)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::renderer::frontend::composer::rect_grid::RectGrid;

    impl RectGrid {
        /// `intersects` tests the queries ran since the last
        /// `start_frame`.
        pub(crate) fn intersect_tests(&self) -> u32 {
            self.intersect_tests.count()
        }
    }
}

#[cfg(test)]
mod tests;
