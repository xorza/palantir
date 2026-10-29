//! A grid of fixed tiles over the viewport, each listing the entries registered over it: the
//! spatial index both the composer's overlap checks ([`RectGrid`]) and its occlusion pruning
//! ([`OcclusionPruner`]) query.
//!
//! [`RectGrid`]: crate::renderer::frontend::composer::rect_grid::RectGrid
//! [`OcclusionPruner`]: crate::renderer::frontend::composer::occlusion_pruner::OcclusionPruner

use glam::UVec2;

/// Physical-pixel size of one tile; an entry registers into every tile it spans. 64 px was the
/// measured optimum on `rect_grid/realistic`.
pub(super) const TILE_SIZE: u32 = 64;

/// Per-tile inline capacity, sized from the densest observed workload (max occupancy 3). A tile
/// past it chains the rest, so pathological UIs pay a pointer walk only in crowded tiles.
pub(super) const TILE_CAP: usize = 8;

/// The most tiles an entry is listed in: one spanning more goes in the large lane, which every
/// query walks, so a viewport-wide entry costs one push, not a push per tile (2040 at 4K).
const LARGE_TILES: u32 = 256;

/// An empty overflow chain.
const NO_OVERFLOW: u32 = u32::MAX;

/// One link of a tile's overflow chain.
#[derive(Clone, Copy, Debug)]
struct OverflowEntry {
    id: u32,
    next: u32,
}

/// The tiles an entry spans, both corners inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TileSpan {
    pub(super) min: UVec2,
    pub(super) max: UVec2,
}

/// A row-major grid of tiles ([`TILE_SIZE`] physical px), each listing the ids registered over
/// it, so a query walks only the tiles it touches. An id spanning tiles is listed in each, so a
/// query may meet it twice; callers stop at the first hit, so that costs only a constant factor.
/// An id spanning more than [`LARGE_TILES`] tiles is listed once, in a lane every query walks.
///
/// Storage is flat SoA (`lens` + fixed `slots` rows), not a `Vec<TinyVec>`: registering and
/// walking are the composer's hottest loops, and TinyVec's tag dispatch cost ~2% of the frame. For
/// the same reason the walks are `#[inline(always)]`: as calls, `rect_grid/realistic` ran 32 %
/// slower (`.notes/research/2026-10-27-gpu-upload-and-binder.md`, "One tile index").
#[derive(Debug, Default)]
pub(super) struct TileIndex {
    cols: u32,
    rows: u32,
    /// Per-tile occupancy (`0..=TILE_CAP`), row-major `lens[ty * cols + tx]`, parallel to
    /// `slots`. Reset via the `touched` walk.
    lens: Vec<u8>,
    slots: Vec<[u16; TILE_CAP]>,
    /// Per-tile head of the overflow chain in [`Self::overflow`], or [`NO_OVERFLOW`]. Parallel
    /// to `lens`, reset by the same `touched` walk.
    overflow_heads: Vec<u32>,
    /// Every tile's ids past its inline row (after [`TILE_CAP`], or too big for `u16`), as
    /// singly linked chains, newest first, in one flat buffer.
    overflow: Vec<OverflowEntry>,
    /// Tiles registered into since the last clear: what [`Self::clear`] walks instead of the
    /// whole grid. A tile is listed when it first goes non-empty. Capacity is retained.
    touched: Vec<u32>,
    /// The ids spanning more than [`LARGE_TILES`] tiles, in insert order.
    large: Vec<u32>,
}

impl TileIndex {
    /// Reshape to cover `viewport` and drop every entry. No allocation when the viewport is
    /// unchanged.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        let cols = viewport.x.div_ceil(TILE_SIZE).max(1);
        let rows = viewport.y.div_ceil(TILE_SIZE).max(1);
        let want = (cols * rows) as usize;
        // Grow-only: a smaller viewport reuses the larger vectors, and spans are clamped so tiles
        // beyond the grid are untouched. `touched` holds absolute indices, so `clear` is
        // independent of the grid shape. Shrinking would make a resize drag sweep every tile.
        if want > self.lens.len() {
            self.lens.resize(want, 0);
            self.slots.resize(want, [0; TILE_CAP]);
            self.overflow_heads.resize(want, NO_OVERFLOW);
        }
        self.cols = cols;
        self.rows = rows;
        self.clear();
    }

    /// Drop every entry, walking only the tiles registered into.
    pub(super) fn clear(&mut self) {
        for &i in &self.touched {
            self.lens[i as usize] = 0;
            self.overflow_heads[i as usize] = NO_OVERFLOW;
        }
        self.touched.clear();
        self.overflow.clear();
        self.large.clear();
    }

    /// The last tile of the grid, the bound a span is clamped to.
    pub(super) const fn last_tile(&self) -> UVec2 {
        UVec2::new(self.cols - 1, self.rows - 1)
    }

    /// The tiles the physical-pixel box `min..max` (exclusive, not empty) covers, clamped to the
    /// grid.
    #[inline(always)]
    pub(super) fn span(&self, min: UVec2, max: UVec2) -> TileSpan {
        debug_assert!(
            max.cmpgt(min).all(),
            "an empty box {min}..{max} spans no tile"
        );
        let last = self.last_tile();
        TileSpan {
            min: (min / TILE_SIZE).min(last),
            max: ((max - UVec2::ONE) / TILE_SIZE).min(last),
        }
    }

    /// List `id` in every tile of `span`, or once in the large lane past [`LARGE_TILES`].
    #[inline(always)]
    pub(super) fn insert(&mut self, id: u32, span: TileSpan) {
        let tiles = span.max - span.min + UVec2::ONE;
        if tiles.x * tiles.y > LARGE_TILES {
            self.large.push(id);
            return;
        }
        let inline = u16::try_from(id).ok();
        for ty in span.min.y..=span.max.y {
            let row = ty * self.cols;
            for tx in span.min.x..=span.max.x {
                let tile = (row + tx) as usize;
                let len = self.lens[tile] as usize;
                let head = self.overflow_heads[tile];
                if len == 0 && head == NO_OVERFLOW {
                    self.touched.push(tile as u32);
                }
                match inline {
                    Some(id) if len < TILE_CAP => {
                        self.slots[tile][len] = id;
                        self.lens[tile] = (len + 1) as u8;
                    }
                    _ => {
                        self.overflow_heads[tile] = self.overflow.len() as u32;
                        self.overflow.push(OverflowEntry { id, next: head });
                    }
                }
            }
        }
    }

    /// Whether `hit` answers `true` for an id listed over tile `tile` (by grid position), in the
    /// tile or the large lane, stopping at the first.
    #[inline]
    pub(super) fn any_in(&self, tile: UVec2, mut hit: impl FnMut(u32) -> bool) -> bool {
        self.any_at((tile.y * self.cols + tile.x) as usize, &mut hit) || self.any_large(hit)
    }

    /// Whether `hit` answers `true` for an id of the large lane.
    #[inline(always)]
    fn any_large(&self, mut hit: impl FnMut(u32) -> bool) -> bool {
        self.large.iter().any(|&id| hit(id))
    }

    /// [`Self::any_in`] by flat tile index.
    #[inline(always)]
    fn any_at(&self, t: usize, mut hit: impl FnMut(u32) -> bool) -> bool {
        let n = self.lens[t] as usize;
        for &id in &self.slots[t][..n] {
            if hit(u32::from(id)) {
                return true;
            }
        }
        let mut link = self.overflow_heads[t];
        while link != NO_OVERFLOW {
            let entry = self.overflow[link as usize];
            if hit(entry.id) {
                return true;
            }
            link = entry.next;
        }
        false
    }

    /// [`Self::any_in`] over every tile of `span`.
    #[inline(always)]
    pub(super) fn any_over(&self, span: TileSpan, mut hit: impl FnMut(u32) -> bool) -> bool {
        for ty in span.min.y..=span.max.y {
            let row = ty * self.cols;
            for tx in span.min.x..=span.max.x {
                if self.any_at((row + tx) as usize, &mut hit) {
                    return true;
                }
            }
        }
        self.any_large(hit)
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::renderer::frontend::composer::tile_index::{NO_OVERFLOW, TileIndex};

    impl TileIndex {
        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            use crate::internals::scratch_report;
            let Self {
                cols: _,
                rows: _,
                lens,
                slots,
                overflow_heads,
                overflow,
                touched,
                large,
            } = self;
            scratch_report::vec_bytes(lens)
                + scratch_report::vec_bytes(slots)
                + scratch_report::vec_bytes(overflow_heads)
                + scratch_report::vec_bytes(overflow)
                + scratch_report::vec_bytes(touched)
                + scratch_report::vec_bytes(large)
        }

        /// Tiles the storage holds, the high-water mark of every viewport; `lens`, `slots` and
        /// `overflow_heads` stay parallel.
        #[cfg(test)]
        pub(crate) fn tile_capacity(&self) -> usize {
            assert!(
                self.lens.len() == self.slots.len()
                    && self.slots.len() == self.overflow_heads.len()
            );
            self.slots.len()
        }

        /// Ids listed inline in tile `tile`, and in its overflow chain.
        pub(crate) fn tile_load(&self, tile: usize) -> [usize; 2] {
            let mut chained = 0;
            let mut link = self.overflow_heads[tile];
            while link != NO_OVERFLOW {
                chained += 1;
                link = self.overflow[link as usize].next;
            }
            [usize::from(self.lens[tile]), chained]
        }
    }
}
