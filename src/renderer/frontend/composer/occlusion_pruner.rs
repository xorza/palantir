//! CPU occlusion pruning: drop quads in a scissor group fully covered by a
//! later opaque quad in the same group.

use crate::common::counters::TestOnly;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::composer::tile_index::{TILE_SIZE, TileIndex, TileSpan};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::quad_row::QuadKey;
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
/// corner, so a query tests only that tile's covers and the index's large
/// lane, skipping those drawn beneath the quad.
///
/// Covers are clamped to the viewport's tiles; the clamp is monotonic, so a
/// clamped corner still lands in range. An index miss only keeps a quad the GPU
/// overdraws, so the index may be conservative, never wrong.
#[derive(Debug, Default)]
pub(super) struct OcclusionPruner {
    opaque_in_group: Vec<Occluder>,
    /// Each tile's covers, by position in `opaque_in_group`.
    tiles: TileIndex,
    contains_tests: TestOnly<Cell<u32>>,
}

const LINEAR_OCCLUDERS: usize = 16;

const TILE: f32 = TILE_SIZE as f32;

impl OcclusionPruner {
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.tiles.start_frame(viewport);
        self.contains_tests.reset();
        self.clear();
    }

    pub(super) fn clear(&mut self) {
        self.opaque_in_group.clear();
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

        // Always at the first occluder with `idx > i`; both ascend.
        let mut cursor = 0;
        let occluders = self.opaque_in_group.len();
        let end = out.quads.len();
        let rows = out.quads.slices_mut();
        let (keys, quads) = (rows.key, rows.instance);
        // Survivors move down to `write`, in order.
        let mut write = start;
        for read in start..end {
            let key = keys[read];
            // Shadows blur past the stored rect; never drop.
            if key != QuadKey::Shadow {
                let i = read - start;
                while cursor < occluders && self.opaque_in_group[cursor].idx as usize <= i {
                    cursor += 1;
                }
                // No later occluder for this or any later quad: the rest survive.
                if cursor >= occluders {
                    keys.copy_within(read..end, write);
                    quads.copy_within(read..end, write);
                    write += end - read;
                    break;
                }
                let shaded = quads[read].header.shaded_rect();
                let covered = if indexed {
                    self.indexed_covers(shaded, cursor)
                } else {
                    (cursor..occluders).any(|at| self.covers(at, shaded))
                };
                if covered {
                    continue;
                }
            }
            if read != write {
                keys[write] = key;
                quads[write] = quads[read];
            }
            write += 1;
        }
        out.quads.truncate(write);
    }

    fn build_index(&mut self) {
        self.tiles.clear();
        for (at, occluder) in self.opaque_in_group.iter().enumerate() {
            let span = self.span_of(occluder.cover);
            self.tiles.insert(at as u32, span);
        }
    }

    fn indexed_covers(&self, rect: Rect, cursor: usize) -> bool {
        // A cover before `cursor` is drawn beneath the quad.
        self.tiles.any_in(self.tile_of(rect.min), |at| {
            at as usize >= cursor && self.covers(at as usize, rect)
        })
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
                tiles,
                contains_tests: _,
            } = self;
            scratch_report::vec_bytes(opaque_in_group) + tiles.scratch_bytes()
        }
    }
}
