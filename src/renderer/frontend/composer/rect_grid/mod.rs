//! Spatial index over paint-rect AABBs for the composer's paint-order overlap checks: the text batches' rects, and higher-kind tiers' once they outgrow a short scan. A flat `Vec<URect>` scan costs every query the whole batch, which dominates compose time in text-dense UIs.
//!
//! Real workloads keep dozens to ~200 rects live; most queries die at the union pre-reject and a survivor walks a few tiles.
//!
//! A union pre-reject plus linear scan still scans too much on surviving queries; a conservative coverage bitmap needs small cells to avoid false-positive flushes and then loses; sorted sweeps, interval trees and BVHs need an O(n log n) rebuild per frame. Exactness is what the grid pays for.

#[cfg(feature = "internals")]
pub(crate) mod bench;

use crate::common::counters::TestOnly;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::tile_index::TileIndex;
use glam::UVec2;
use std::cell::Cell;

/// Spatial index over the open batch's text-rect AABBs: each rect is listed in a [`TileIndex`]
/// over every tile it overlaps, and a query walks only the tiles its rect overlaps.
#[derive(Debug, Default)]
pub(super) struct RectGrid {
    tiles: TileIndex,
    rects: Vec<URect>,
    intersect_tests: TestOnly<Cell<u32>>,
    /// Union AABB of every rect: an O(1) pre-reject for [`Self::any_overlap`] that skips the tile walk. Zero-sized means empty.
    pub(super) union: URect,
}

impl RectGrid {
    /// Reshape to cover `viewport` and reset all state, before the first push of new scene output. No allocation when the viewport is unchanged.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.tiles.start_frame(viewport);
        self.intersect_tests.reset();
        self.rects.clear();
        self.union = URect::ZERO;
    }

    /// Drop every registered rect, walking only the tiles pushed to this frame.
    pub(super) fn clear(&mut self) {
        self.tiles.clear();
        self.rects.clear();
        self.union = URect::ZERO;
    }

    pub(super) fn push(&mut self, r: URect) {
        if r.is_paint_empty() {
            return;
        }
        let idx = self.rects.len() as u32;
        self.rects.push(r);
        self.union = self.union.union(r);
        self.tiles.insert(idx, self.tiles.span(r.min, r.max()));
    }

    /// `true` if any registered rect intersects `q`, returning on the first hit. Walks every tile in `q`'s range and checks each tile's rect list.
    pub(super) fn any_overlap(&self, q: URect) -> bool {
        if q.is_paint_empty() || !self.union.intersects(q) {
            return false;
        }
        self.tiles
            .any_over(self.tiles.span(q.min, q.max()), |rect| self.hit(rect, q))
    }

    fn hit(&self, rect: u32, q: URect) -> bool {
        self.intersect_tests.bump_shared();
        self.rects[rect as usize].intersects(q)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::rect_grid::RectGrid;

    impl RectGrid {
        pub(crate) fn intersect_tests(&self) -> u32 {
            self.intersect_tests.count()
        }

        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                tiles,
                rects,
                intersect_tests: _,
                union: _,
            } = self;
            tiles.scratch_bytes() + scratch_report::vec_bytes(rects)
        }

        pub(crate) fn tile_load(&self, tile: usize) -> [usize; 2] {
            self.tiles.tile_load(tile)
        }

        pub(crate) fn tile_capacity(&self) -> usize {
            self.tiles.tile_capacity()
        }
    }
}

#[cfg(test)]
mod tests;
