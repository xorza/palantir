//! Spatial index over paint-rect AABBs for the composer's paint-order overlap checks: the text batches' rects, and higher-kind tiers' once they outgrow a short scan. A flat `Vec<URect>` scan costs every query the whole batch, which dominates compose time in text-dense UIs.
//!
//! Real workloads keep dozens to ~200 rects live; most queries die at the union pre-reject and a survivor walks a few tiles.
//!
//! A union pre-reject plus linear scan still scans too much on surviving queries; a conservative coverage bitmap needs small cells to avoid false-positive flushes and then loses; sorted sweeps, interval trees and BVHs need an O(n log n) rebuild per frame. Exactness is what the grid pays for.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::common::counters::TestOnly;
use crate::primitives::geometry::urect::URect;
use glam::UVec2;
use std::cell::Cell;

/// Physical-pixel size of one tile in [`RectGrid`]; each text rect registers into every tile it overlaps. 64 px was the measured optimum on `rect_grid/realistic`.
pub(super) const TILE_SIZE: u32 = 64;

/// An empty overflow chain.
const NO_OVERFLOW: u32 = u32::MAX;

/// One link of a tile's overflow chain.
#[derive(Clone, Copy, Debug)]
struct OverflowEntry {
    rect: u32,
    next: u32,
}

/// Per-tile inline capacity, sized from the densest observed workload (max occupancy 3). A tile past it chains the rest, so pathological UIs pay a pointer walk only in crowded tiles.
pub(super) const TILE_CAP: usize = 8;

/// Spatial index over the open batch's text-rect AABBs: a row-major grid of tiles ([`TILE_SIZE`] phys px) where a query walks only the tiles its rect overlaps. A rect spanning tiles may be visited twice; queries early-exit, so that costs only constant-factor false positives.
///
/// Tile storage is flat SoA (`lens` + fixed `slots` rows), not a `Vec<TinyVec>`: `push`/`any_overlap` are the composer's hottest loops and TinyVec's tag dispatch cost ~2% of the frame.
#[derive(Debug, Default)]
pub(super) struct RectGrid {
    cols: u32,
    rows: u32,
    /// Per-tile occupancy (`0..=TILE_CAP`), row-major `lens[ty * cols + tx]`, parallel to `slots`. Reset per batch via the `touched` walk.
    lens: Vec<u8>,
    slots: Vec<[u16; TILE_CAP]>,
    /// Per-tile head of the overflow chain in [`Self::overflow`], or [`NO_OVERFLOW`]. Parallel to `lens`, reset by the same `touched` walk.
    overflow_heads: Vec<u32>,
    /// Every tile's rects past its inline row (after [`TILE_CAP`], or with an index too big for `u16`), as singly linked chains, newest first, in one flat buffer.
    overflow: Vec<OverflowEntry>,
    /// Indices (into `lens`/`slots`) pushed to this frame: what [`Self::clear`] walks instead of the whole grid. A tile is recorded when it first goes non-empty. Capacity is retained across frames.
    touched: Vec<u32>,
    rects: Vec<URect>,
    intersect_tests: TestOnly<Cell<u32>>,
    /// Union AABB of every rect: an O(1) pre-reject for [`Self::any_overlap`] that skips the tile walk. Zero-sized means empty.
    pub(super) union: URect,
}

impl RectGrid {
    /// Reshape to cover `viewport` and reset all state, once per frame at compose start. No allocation when the viewport is unchanged.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only: a smaller viewport reuses the larger vectors, and `push` clamps indices so tiles beyond the grid are untouched. `touched` holds absolute indices, so `clear` is independent of the grid shape. Shrinking would make a resize drag sweep every tile each frame.
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

    /// Drop every registered rect, walking only the tiles pushed to this frame (`touched`), not the full grid.
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

    /// `true` if any registered rect intersects `q`, returning on the first hit. Walks every tile in `q`'s range and checks each tile's rect list.
    pub(super) fn any_overlap(&self, q: URect) -> bool {
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
                    if self.hit(u32::from(i), q) {
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
        pub(crate) fn intersect_tests(&self) -> u32 {
            self.intersect_tests.count()
        }
    }
}

#[cfg(test)]
mod tests;
