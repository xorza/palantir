//! CPU occlusion pruning: drop quads in a scissor group fully covered by a
//! later opaque quad in the same group.

use crate::common::counters::TestOnly;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::composer::rect_grid::TILE_SIZE;
use crate::renderer::render_buffer::RenderBuffer;
use glam::{UVec2, Vec2};
use std::cell::Cell;

#[derive(Clone, Copy, Debug)]
struct Occluder {
    idx: u32,
    cover: Rect,
}

/// Per-group scratch: the solid-opaque occluders of the in-flight group, used
/// to drop earlier quads they fully cover.
///
/// Each occluder carries a **cover rect**: the largest axis-aligned rect with
/// guaranteed full opaque coverage. Only pixel-aligned fast-path quads use
/// their full rect; others are inset for corners, translucent strokes and AA.
///
/// # Cost
///
/// Up to [`LINEAR_OCCLUDERS`] occluders are scanned linearly. Beyond that a
/// per-tile index is used: a cover containing a quad contains its top-left
/// corner, so a query tests only that tile's chain, newest first, and stops at
/// the first cover drawn beneath the quad. Covers spanning more than
/// [`LARGE_COVER_TILES`] tiles are tested by every query instead.
///
/// Covers are clamped to the viewport's tiles; the clamp is monotonic, so a
/// clamped corner still lands in range. An index miss only keeps a quad the GPU
/// overdraws, so the index may be conservative, never wrong.
#[derive(Debug, Default)]
pub(super) struct OcclusionPruner {
    opaque_in_group: Vec<Occluder>,
    drop_indices: Vec<u32>,
    last_tile: UVec2,
    /// Each tile's newest link in [`Self::cover_links`], or [`NO_LINK`]. Grow-only,
    /// reset through [`Self::touched_tiles`].
    tile_heads: Vec<u32>,
    touched_tiles: Vec<u32>,
    cover_links: Vec<CoverLink>,
    /// Positions in `opaque_in_group` of covers spanning more than
    /// [`LARGE_COVER_TILES`] tiles, ascending.
    large_covers: Vec<u32>,
    contains_tests: TestOnly<Cell<u32>>,
}

#[derive(Clone, Copy, Debug)]
struct CoverLink {
    at: u32,
    next: u32,
}

const NO_LINK: u32 = u32::MAX;

const LINEAR_OCCLUDERS: usize = 16;

/// The most tiles a cover registers in before every query tests it instead.
const LARGE_COVER_TILES: u32 = 256;

const TILE: f32 = TILE_SIZE as f32;

#[derive(Clone, Copy, Debug)]
struct TileSpan {
    min: UVec2,
    max: UVec2,
}

impl OcclusionPruner {
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.last_tile = UVec2::new(
            viewport.x.div_ceil(TILE_SIZE).max(1) - 1,
            viewport.y.div_ceil(TILE_SIZE).max(1) - 1,
        );
        let tiles = ((self.last_tile.x + 1) * (self.last_tile.y + 1)) as usize;
        if tiles > self.tile_heads.len() {
            self.tile_heads.resize(tiles, NO_LINK);
        }
        self.contains_tests.reset();
        self.clear();
    }

    pub(super) fn clear(&mut self) {
        self.opaque_in_group.clear();
        self.drop_indices.clear();
    }

    pub(super) fn record_opaque(&mut self, idx: u32, cover: Rect) {
        self.opaque_in_group.push(Occluder { idx, cover });
    }

    /// Drop quads in the in-flight group (`out.quads[quads_cursor..]`) fully
    /// covered by a later opaque quad, compacting in place and keeping survivor order.
    pub(super) fn prune(&mut self, out: &mut RenderBuffer, quads_cursor: u32) {
        let start = quads_cursor as usize;
        if out.quads.len() - start < 2 || self.opaque_in_group.is_empty() {
            return;
        }
        let indexed = self.opaque_in_group.len() > LINEAR_OCCLUDERS;
        if indexed {
            self.build_index();
        }

        self.drop_indices.clear();
        // Always at the first occluder with `idx > i`; both ascend.
        let mut cursor = 0;
        let occluders = self.opaque_in_group.len();
        for (i, q) in out.quads[start..].iter().enumerate() {
            // Shadows blur past the stored rect; never drop.
            if q.fill_kind.is_shadow() {
                continue;
            }
            while cursor < occluders && self.opaque_in_group[cursor].idx as usize <= i {
                cursor += 1;
            }
            // No later occluder for this or any later `i`.
            if cursor >= occluders {
                break;
            }
            let shaded = q.shaded_rect();
            let covered = if indexed {
                self.indexed_covers(shaded, cursor)
            } else {
                (cursor..occluders).any(|at| self.covers(at, shaded))
            };
            if covered {
                self.drop_indices.push(i as u32);
            }
        }
        if self.drop_indices.is_empty() {
            return;
        }
        // Walk forward, copying survivors down; `drop_indices` is ascending.
        let mut drop_iter = self.drop_indices.iter().copied().peekable();
        let mut write = start;
        for read in start..out.quads.len() {
            let rel = (read - start) as u32;
            if drop_iter.peek().copied() == Some(rel) {
                drop_iter.next();
                continue;
            }
            if read != write {
                out.quads[write] = out.quads[read];
            }
            write += 1;
        }
        out.quads.truncate(write);
    }

    fn build_index(&mut self) {
        for &tile in &self.touched_tiles {
            self.tile_heads[tile as usize] = NO_LINK;
        }
        self.touched_tiles.clear();
        self.cover_links.clear();
        self.large_covers.clear();
        for (at, occluder) in self.opaque_in_group.iter().enumerate() {
            let span = self.span_of(occluder.cover);
            let tiles = span.max - span.min + UVec2::ONE;
            if tiles.x * tiles.y > LARGE_COVER_TILES {
                self.large_covers.push(at as u32);
                continue;
            }
            for y in span.min.y..=span.max.y {
                for x in span.min.x..=span.max.x {
                    let tile = self.tile_index(UVec2::new(x, y));
                    let head = &mut self.tile_heads[tile as usize];
                    if *head == NO_LINK {
                        self.touched_tiles.push(tile);
                    }
                    self.cover_links.push(CoverLink {
                        at: at as u32,
                        next: *head,
                    });
                    *head = (self.cover_links.len() - 1) as u32;
                }
            }
        }
    }

    fn indexed_covers(&self, rect: Rect, cursor: usize) -> bool {
        let tile = self.tile_index(self.tile_of(rect.min));
        let mut link = self.tile_heads[tile as usize];
        while link != NO_LINK {
            let CoverLink { at, next } = self.cover_links[link as usize];
            // Newest first: the rest of the chain is drawn beneath the quad.
            if (at as usize) < cursor {
                break;
            }
            if self.covers(at as usize, rect) {
                return true;
            }
            link = next;
        }
        let first = self
            .large_covers
            .partition_point(|&at| (at as usize) < cursor);
        (first..self.large_covers.len()).any(|at| self.covers(self.large_covers[at] as usize, rect))
    }

    fn covers(&self, at: usize, rect: Rect) -> bool {
        self.contains_tests.bump_shared();
        self.opaque_in_group[at].cover.contains_rect(rect)
    }

    /// The tiles a quad `rect` contains can start in. A quad has area, so its
    /// corner lies strictly before the cover's far edges: a cover edge on a tile
    /// boundary stays out of the next tile.
    fn span_of(&self, rect: Rect) -> TileSpan {
        let last =
            ((rect.max() / TILE).ceil() - Vec2::ONE).clamp(Vec2::ZERO, self.last_tile.as_vec2());
        let span = TileSpan {
            min: self.tile_of(rect.min),
            max: last.as_uvec2(),
        };
        debug_assert!(
            span.min.cmple(span.max).all(),
            "an empty cover {rect:?} reached the index",
        );
        span
    }

    fn tile_of(&self, point: Vec2) -> UVec2 {
        (point / TILE)
            .floor()
            .clamp(Vec2::ZERO, self.last_tile.as_vec2())
            .as_uvec2()
    }

    const fn tile_index(&self, tile: UVec2) -> u32 {
        tile.y * (self.last_tile.x + 1) + tile.x
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::renderer::frontend::composer::occlusion::OcclusionPruner;

    impl OcclusionPruner {
        /// `contains_rect` tests the prune ran this frame.
        pub(crate) fn contains_tests(&self) -> u32 {
            self.contains_tests.count()
        }
    }
}
