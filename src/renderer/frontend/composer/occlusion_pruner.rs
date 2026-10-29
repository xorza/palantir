//! CPU occlusion pruning: drop quads in a scissor group fully covered by a
//! later opaque quad in the same group.

use crate::common::counters::TestOnly;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::composer::tile_index::{TILE_SIZE, TileIndex, TileSpan};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::PaneSource;
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
/// [`TileIndex`] is used: a cover containing a quad contains its top-left
/// corner, so a query tests only that tile's covers, skipping those drawn
/// beneath the quad. Covers spanning more than [`LARGE_COVER_TILES`] tiles are
/// tested by every query instead.
///
/// Covers are clamped to the viewport's tiles; the clamp is monotonic, so a
/// clamped corner still lands in range. An index miss only keeps a quad the GPU
/// overdraws, so the index may be conservative, never wrong.
#[derive(Debug, Default)]
pub(super) struct OcclusionPruner {
    opaque_in_group: Vec<Occluder>,
    drop_idxs: Vec<u32>,
    /// Each tile's covers, by position in `opaque_in_group`.
    tiles: TileIndex,
    /// Positions in `opaque_in_group` of covers spanning more than
    /// [`LARGE_COVER_TILES`] tiles, ascending.
    large_covers: Vec<u32>,
    contains_tests: TestOnly<Cell<u32>>,
}

const LINEAR_OCCLUDERS: usize = 16;

/// The most tiles a cover registers in before every query tests it instead.
const LARGE_COVER_TILES: u32 = 256;

const TILE: f32 = TILE_SIZE as f32;

impl OcclusionPruner {
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.tiles.start_frame(viewport);
        self.contains_tests.reset();
        self.clear();
    }

    pub(super) fn clear(&mut self) {
        self.opaque_in_group.clear();
        self.drop_idxs.clear();
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

        self.drop_idxs.clear();
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
                // A pane that shows nothing must not capture, nor keep a result that goes
                // stale while it is hidden.
                if q.fill_kind.is_backdrop() {
                    out.backdrops[q.fill_lut_row.0 as usize].source = PaneSource::Hidden;
                }
                self.drop_idxs.push(i as u32);
            }
        }
        if self.drop_idxs.is_empty() {
            return;
        }
        // Walk forward, copying survivors down; `drop_idxs` is ascending.
        let mut drop_iter = self.drop_idxs.iter().copied().peekable();
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
        self.tiles.clear();
        self.large_covers.clear();
        for (at, occluder) in self.opaque_in_group.iter().enumerate() {
            let span = self.span_of(occluder.cover);
            let tiles = span.max - span.min + UVec2::ONE;
            if tiles.x * tiles.y > LARGE_COVER_TILES {
                self.large_covers.push(at as u32);
                continue;
            }
            self.tiles.insert(at as u32, span);
        }
    }

    fn indexed_covers(&self, rect: Rect, cursor: usize) -> bool {
        // A cover before `cursor` is drawn beneath the quad.
        if self.tiles.any_in(self.tile_of(rect.min), |at| {
            at as usize >= cursor && self.covers(at as usize, rect)
        }) {
            return true;
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
        let last = ((rect.max() / TILE).ceil() - Vec2::ONE)
            .clamp(Vec2::ZERO, self.tiles.last_tile().as_vec2());
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
            .clamp(Vec2::ZERO, self.tiles.last_tile().as_vec2())
            .as_uvec2()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::occlusion_pruner::OcclusionPruner;

    impl OcclusionPruner {
        /// `contains_rect` tests the prune ran this frame.
        pub(crate) fn contains_tests(&self) -> u32 {
            self.contains_tests.count()
        }

        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                opaque_in_group,
                drop_idxs,
                tiles,
                large_covers,
                contains_tests: _,
            } = self;
            scratch_report::vec_bytes(opaque_in_group)
                + scratch_report::vec_bytes(drop_idxs)
                + tiles.scratch_bytes()
                + scratch_report::vec_bytes(large_covers)
        }
    }
}
