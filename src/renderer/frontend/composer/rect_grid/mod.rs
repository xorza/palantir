//! Spatial index over paint-rect AABBs for the composer's paint-order overlap checks: the text batches' rects and each higher-kind tier's. A flat `Vec<URect>` scan costs every query the whole set, which dominates compose time in text-dense UIs.
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

/// The most rects a set scans without its index: past it a query lists the rects not yet indexed
/// in the [`TileIndex`] and tests only the tiles it touches.
const LINEAR_RECTS: usize = 32;

/// A set of paint-rect AABBs that answers whether a rect overlaps any of them: the composer's one
/// overlap index, for the text batches and every higher-kind tier alike. A query scans up to
/// [`LINEAR_RECTS`] rects; past that it indexes the rects pushed since the last query and walks
/// only the tiles it overlaps. Indexed by queries, not pushes, since most sets are never queried
/// at length. Refused: an eager index for text and a lazy one per tier, two policies over one
/// [`TileIndex`] tuned and tested apart.
#[derive(Debug, Default)]
pub(super) struct RectGrid {
    rects: Vec<URect>,
    /// Union AABB of every rect: an O(1) pre-reject for [`Self::any_overlap`]. Zero-sized means
    /// empty.
    union: URect,
    /// The rects `rects[..indexed]`, by tile, as indices into `rects`. Shaped by the first index,
    /// not by [`Self::start_frame`]: most sets never pass [`LINEAR_RECTS`], and shaping each would
    /// allocate a viewport of tiles nothing reads.
    tiles: TileIndex,
    indexed: usize,
    /// The viewport [`Self::start_frame`] gave, until the first index shapes `tiles` to it.
    unshaped: Option<UVec2>,
    intersect_tests: TestOnly<Cell<u32>>,
}

impl RectGrid {
    /// Drop every rect, and give the tiles the `viewport` they take on their first index; called
    /// before new scene output.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.clear();
        self.unshaped = Some(viewport);
        self.intersect_tests.reset();
    }

    /// Drop every rect, walking only the tiles indexed this frame.
    pub(super) fn clear(&mut self) {
        self.rects.clear();
        self.union = URect::ZERO;
        if self.indexed > 0 {
            self.tiles.clear();
            self.indexed = 0;
        }
    }

    /// Add `r`; a rect that paints nothing overlaps nothing, so it is not kept.
    pub(super) fn push(&mut self, r: URect) {
        if r.is_paint_empty() {
            return;
        }
        self.rects.push(r);
        self.union = self.union.union(r);
    }

    /// The union of every rect; zero-sized while the set is empty.
    pub(super) const fn union(&self) -> URect {
        self.union
    }

    /// `true` if any rect intersects `q`, returning on the first hit. The union reject inline,
    /// since every draw asks it of the sets above its own; the scan behind it out of line.
    #[inline]
    pub(super) fn any_overlap(&mut self, q: URect) -> bool {
        !q.is_paint_empty() && self.union.intersects(q) && self.any_overlap_inside(q)
    }

    #[inline(never)]
    fn any_overlap_inside(&mut self, q: URect) -> bool {
        if self.rects.len() <= LINEAR_RECTS {
            return self.rects.iter().any(|r| r.intersects(q));
        }
        self.index_pending();
        self.tiles.any_over(self.tiles.span(q.min, q.max()), |at| {
            self.intersect_tests.bump_shared();
            self.rects[at as usize].intersects(q)
        })
    }

    /// List every rect pushed since the last index in the tiles, shaping them first in a frame.
    fn index_pending(&mut self) {
        if let Some(viewport) = self.unshaped.take() {
            self.tiles.start_frame(viewport);
        }
        for (at, rect) in self.rects.iter().enumerate().skip(self.indexed) {
            self.tiles
                .insert(at as u32, self.tiles.span(rect.min, rect.max()));
        }
        self.indexed = self.rects.len();
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::rect_grid::RectGrid;

    impl RectGrid {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                rects,
                union: _,
                tiles,
                indexed: _,
                unshaped: _,
                intersect_tests: _,
            } = self;
            tiles.scratch_bytes() + scratch_report::vec_bytes(rects)
        }

        /// The bytes the tiles hold: zero for a set never indexed.
        pub(crate) fn tile_bytes(&self) -> usize {
            self.tiles.scratch_bytes()
        }

        /// The rect tests the indexed queries ran this frame.
        pub(crate) fn intersect_tests(&self) -> u32 {
            self.intersect_tests.count()
        }

        pub(crate) fn len(&self) -> usize {
            self.rects.len()
        }

        /// Tile `tile`'s inline and chained rect counts, once every rect is indexed.
        pub(crate) fn tile_load(&mut self, tile: usize) -> [usize; 2] {
            self.index_pending();
            self.tiles.tile_load(tile)
        }

        /// The tile storage, once the tiles are shaped.
        pub(crate) fn tile_capacity(&mut self) -> usize {
            self.index_pending();
            self.tiles.tile_capacity()
        }
    }
}

#[cfg(test)]
mod tests;
