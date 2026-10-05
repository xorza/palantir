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
/// tests only the covers registered in that one tile. Each tile holds its
/// covers as a chain, newest first: covers register in occluder order, so
/// a query walks its tile's chain only while the covers are still drawn
/// above the quad, and the index is built in one pass with no sort. A cover spanning
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
    /// Each tile's newest link in [`Self::cover_links`], or [`NO_LINK`].
    /// Sized to the viewport's tiles, grow-only, and reset through
    /// [`Self::touched_tiles`] rather than whole.
    tile_heads: Vec<u32>,
    /// The tiles whose head the current index set.
    touched_tiles: Vec<u32>,
    /// Every small cover once per tile it overlaps, chained per tile.
    cover_links: Vec<CoverLink>,
    /// Positions in `opaque_in_group` of the covers spanning more than
    /// [`LARGE_COVER_TILES`] tiles, ascending.
    large_covers: Vec<u32>,
    /// `contains_rect` tests this frame.
    contains_tests: TestOnly<Cell<u32>>,
}

/// One small cover in one tile's chain.
#[derive(Clone, Copy, Debug)]
struct CoverLink {
    /// The cover's position in `opaque_in_group`.
    at: u32,
    /// The tile's previous link, or [`NO_LINK`].
    next: u32,
}

/// The end of a tile's chain.
const NO_LINK: u32 = u32::MAX;

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
        let tiles = ((self.last_tile.x + 1) * (self.last_tile.y + 1)) as usize;
        if tiles > self.tile_heads.len() {
            self.tile_heads.resize(tiles, NO_LINK);
        }
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
    /// - For each quad at slice index `i`, its painted extent is
    ///   [`Quad::shaded_rect`](crate::renderer::quad::Quad::shaded_rect): the pixels its edge ramp reaches past the
    ///   rect, since a quad's border is an inner-edge annulus and adds
    ///   nothing outside it. Drop the quad if some occluder with `idx > i`
    ///   (drawn on top) has `cover.contains_rect` of that extent.
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

    /// Whether an occluder at position `cursor` or later covers `rect`,
    /// through the index.
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
