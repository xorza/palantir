//! CPU occlusion pruning: drop quads in a scissor group that are fully
//! covered by a later opaque quad in the same group. Pure prune — no
//! pipeline or shader changes.

use crate::common::counters::TestOnly;
use crate::primitives::geometry::rect::Rect;
use crate::renderer::frontend::composer::rect_grid::TILE_SIZE;
use crate::renderer::render_buffer::RenderBuffer;
use glam::{UVec2, Vec2};
use std::cell::Cell;

/// One opaque occluder in the in-flight group. See [`OcclusionPruner`]
/// for the cover-rect contract.
#[derive(Clone, Copy, Debug)]
struct Occluder {
    /// Index inside the in-flight group's quad slice
    /// (`out.quads[quads_cursor + idx]`).
    idx: u32,
    /// Largest axis-aligned rect with full opaque coverage. Used as the
    /// left-hand side of `Rect::contains_rect(occludee.rect)` in the
    /// prune sweep.
    cover: Rect,
}

/// Per-group occlusion-prune scratch. Accumulates the solid-opaque
/// occluders pushed into the in-flight group's quad slice, then drops
/// every earlier quad those occluders fully cover.
///
/// Each [`Occluder`] pairs the quad's slice-relative index (for
/// "drawn-on-top" ordering — only indices `> i` can occlude quad `i`)
/// with its **cover rect**: the largest axis-aligned rect guaranteed to
/// receive full opaque coverage. Only pixel-aligned fast-path quads use
/// their full rect. Other covers are inset for corners, translucent
/// inner-edge strokes, and the SDF's half-pixel AA transition.
///
/// # Cost
///
/// A group of at most [`LINEAR_OCCLUDERS`] occluders is scanned
/// linearly, which bounds each query at that many tests. A larger one is
/// indexed: a cover that contains a quad contains the quad's top-left
/// corner, so it overlaps the tile that corner falls in, and a query
/// tests only the covers registered in that one tile. A cover spanning
/// more than [`LARGE_COVER_TILES`] tiles is kept apart and tested by
/// every query instead, so no cover registers in more tiles than that —
/// and each such cover is an opaque area of at least that many tiles,
/// which the GPU pays to fill anyway.
///
/// The covers are clamped to the viewport's tiles. A clamped corner
/// still lands in a clamped cover's tile range, because the clamp is
/// monotonic. An index miss only keeps a quad the GPU then overdraws, so
/// the index may be conservative but never has to be complete.
#[derive(Debug, Default)]
pub(super) struct OcclusionPruner {
    /// Solid-opaque occluders in the in-flight group, in push order
    /// (ascending `idx`).
    opaque_in_group: Vec<Occluder>,
    /// Indices (relative to the group's quad cursor) marked for removal
    /// by the prune sweep. Sorted ascending by construction.
    drop_indices: Vec<u32>,
    /// The last tile of the viewport on each axis.
    last_tile: UVec2,
    /// One key per tile a small cover overlaps: the tile index in the
    /// high half, the occluder's position in `opaque_in_group` in the
    /// low half. Sorted, so each tile's covers are one run in occluder
    /// order.
    cover_tiles: Vec<u64>,
    /// Positions in `opaque_in_group` of the covers spanning more than
    /// [`LARGE_COVER_TILES`] tiles, ascending.
    large_covers: Vec<u32>,
    /// `contains_rect` tests this frame.
    contains_tests: TestOnly<Cell<u32>>,
}

/// The largest group the prune scans without an index.
const LINEAR_OCCLUDERS: usize = 16;

/// The most tiles a cover registers in before it is tested by every
/// query instead.
const LARGE_COVER_TILES: u32 = 256;

/// Physical-pixel side of one index tile: the size the rect grid
/// measured best for rects of UI scale.
const TILE: f32 = TILE_SIZE as f32;

/// The inclusive range of tiles a rect overlaps.
#[derive(Clone, Copy, Debug)]
struct TileSpan {
    min: UVec2,
    max: UVec2,
}

impl OcclusionPruner {
    /// Size the index to `viewport` and reset all scratch. Called at
    /// compose start.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        self.last_tile = UVec2::new(
            viewport.x.div_ceil(TILE_SIZE).max(1) - 1,
            viewport.y.div_ceil(TILE_SIZE).max(1) - 1,
        );
        self.contains_tests.reset();
        self.clear();
    }

    /// Reset the group scratch — called at each group flush.
    pub(super) fn clear(&mut self) {
        self.opaque_in_group.clear();
        self.drop_indices.clear();
    }

    /// Record a solid-opaque quad's cover rect at its group-slice index
    /// `idx` so the prune sweep can drop earlier quads contained in it.
    pub(super) fn record_opaque(&mut self, idx: u32, cover: Rect) {
        self.opaque_in_group.push(Occluder { idx, cover });
    }

    /// Drop quads in the in-flight group (`out.quads[quads_cursor..]`)
    /// that are fully covered by a later opaque quad in the same group.
    ///
    /// Preconditions:
    /// - `out.quads[quads_cursor..]` is the in-flight group's contiguous
    ///   slice (composer's flush boundary contract).
    /// - `opaque_in_group` holds an entry for every solid-opaque quad
    ///   pushed into the slice whose cover survived the composer's
    ///   corner, stroke, and AA rules, in push order (ascending `idx`).
    ///
    /// Behaviour:
    /// - For each quad at slice index `i`, its painted extent is `q.rect`:
    ///   a quad's border is an inner-edge annulus, so it adds nothing
    ///   outside the rect. Drop it if some occluder with `idx > i` (drawn
    ///   on top) has `cover.contains_rect(q.rect)`.
    /// - Shadows (`FillKind::is_shadow`) are never dropped — their visual
    ///   blur extends past the stored rect.
    /// - Compacts in place via copy-down; preserves survivor order.
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
        // Cursor into the occluders advancing in lockstep with `i`: it's
        // always positioned at the first occluder with `idx > i`. Both
        // ascend, so the cursor only moves forward.
        let mut cursor = 0;
        let occluders = self.opaque_in_group.len();
        for (i, q) in out.quads[start..].iter().enumerate() {
            // Shadows paint past the stored rect by blur sigma (no
            // closed-form extent we can test cheaply) — never drop.
            if q.fill_kind.is_shadow() {
                continue;
            }
            while cursor < occluders && self.opaque_in_group[cursor].idx as usize <= i {
                cursor += 1;
            }
            // No later occluder exists for this `i`, nor for any later
            // one.
            if cursor >= occluders {
                break;
            }
            // `q.rect` is the painted extent: quad_pipeline/shader.wgsl borders are
            // inner-edge, and the shared ½px AA fringe is what every
            // cover's AA inset answers.
            let covered = if indexed {
                self.indexed_covers(q.rect, cursor)
            } else {
                (cursor..occluders).any(|at| self.covers(at, q.rect))
            };
            if covered {
                self.drop_indices.push(i as u32);
            }
        }
        if self.drop_indices.is_empty() {
            return;
        }
        // Compact in place: walk forward, copy survivors down. The
        // drop list is sorted ascending by construction.
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

    /// Register every small cover in the tiles it overlaps, and set the
    /// large ones apart.
    fn build_index(&mut self) {
        self.cover_tiles.clear();
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
                    self.cover_tiles
                        .push(u64::from(self.tile_index(UVec2::new(x, y))) << 32 | at as u64);
                }
            }
        }
        self.cover_tiles.sort_unstable();
    }

    /// Whether an occluder at position `cursor` or later covers `rect`,
    /// through the index.
    fn indexed_covers(&self, rect: Rect, cursor: usize) -> bool {
        let tile = u64::from(self.tile_index(self.tile_of(rect.min)));
        let first = self
            .cover_tiles
            .partition_point(|&key| key < (tile << 32 | cursor as u64));
        for at in first..self.cover_tiles.len() {
            let key = self.cover_tiles[at];
            if key >> 32 != tile {
                break;
            }
            if self.covers(key as u32 as usize, rect) {
                return true;
            }
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

    /// The tiles a quad `rect` contains can start in. A quad has area, so
    /// its corner lies strictly before the cover's far edges: the span
    /// ends at the tile holding the last point before them, and a cover
    /// whose edge sits on a tile boundary stays out of the tile past it.
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

    /// The tile `point` falls in, clamped to the viewport's.
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
