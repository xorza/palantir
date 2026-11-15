//! One plane's cascade columns.

use crate::cascade::blur_plan::BlurPlan;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::paint::PaintArena;
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::common::content_hash::ContentHash;
use crate::common::non_max::NonMaxU32;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::scene::tree::Tree;
use crate::scene::tree::backdrop_entry::BackdropEntry;
use crate::scene::tree::iter::{TreeItem, TreeItems};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::record::ShapeRecord;

/// All per-plane cascade state. `cascade_inputs`, `subtree_paint_rects` and
/// `paint_arena` are produced together by
/// [`CascadeEngine::run_tree`](crate::cascade::engine::CascadeEngine::run_tree),
/// retained together between frames, and read together by the damage diff and
/// encoder.
///
/// ## Columnar split
///
/// Per-node data is divided by who reads what together:
///
/// - [`Self::cascade_inputs`] is the only per-node hot-path datum: the encoder
///   reads `invisible()` per node and damage compares the full u64. At 8
///   B/node both walks stay cache-dense.
/// - [`Self::subtree_paint_rects`] answers "what does this subtree paint" for
///   the encoder's cull and damage's moved-subtree pushes, in one column so the
///   answer cannot depend on the asker.
/// - [`Self::subtree_ends`] serves
///   [`Cascade::is_within`](crate::cascade::Cascade::is_within) ancestry
///   lookups: sparse random access that must not fatten the walked columns.
/// - [`Self::paint_arena`] holds per-paint-row data, read only on damage's
///   per-shape legs, behind a `node_spans[i]` indirection its subtree-skip
///   fast path avoids.
/// - [`Self::arena_hashes`] stamps the retained `paint_arena` rows with the
///   rollup they were built from.
/// - [`Self::hit_rows`] is read only by the refresh walks, to rewrite a
///   node's hit row in place.
#[derive(Debug, Default)]
pub(crate) struct PlaneCascade {
    /// Per-node cascade input fingerprint, indexed like `Tree::records`:
    /// ancestor state plus own arranged rect hash, with the cascade-resolved
    /// `invisible` bit high (see [`CascadeInputHash`]). Damage pairs the full
    /// u64 with `Tree.rollups.subtree[i]` for its subtree-skip fast path.
    pub(crate) cascade_inputs: Vec<CascadeInputHash>,
    /// Per-node subtree paint rect: the node's own paint extent rolled up with
    /// every descendant's, computed inline in
    /// [`CascadeEngine::run_tree`](crate::cascade::engine::CascadeEngine::run_tree).
    ///
    /// Read by the encoder's viewport and damage culls, which must consider
    /// overhanging descendants (Canvas-positioned children, negative-margin
    /// shapes), and by damage for a moved subtree's extent and a child
    /// marker's extent in the order-inversion check.
    ///
    /// Invisible subtrees seed with `Rect::ZERO`, so a hidden subtree neither
    /// blocks ancestors' culls nor damages anything when it moves. A clip-only
    /// container's visible rect is here and in no row, so the answer only ever
    /// covers more than the rows do.
    pub(crate) subtree_paint_rects: Vec<Rect>,
    /// Per-node pre-order subtree end (`Tree`'s `subtree_end`, grid flag
    /// stripped), snapshotted so ancestry queries can run against the frozen
    /// cascade during the next record, while the live tree is rebuilt.
    pub(super) subtree_ends: Vec<u32>,
    /// Unified paint arena (rows plus per-node spans).
    pub(crate) paint_arena: PaintArena,
    /// Per-node `Tree.rollups.subtree` the retained [`Self::paint_arena`] rows
    /// were built from: the per-node half of the validity gate whose
    /// whole-forest half is [`Cascade::key`](crate::cascade::Cascade::key). An
    /// incremental repair recomputes a node where this disagrees with the live
    /// rollup or its cascade input moved, and re-stamps it.
    ///
    /// Not mergeable with the damage engine's snapshot of the same rollup
    /// ([`NodeSnapshot::subtree_hash`](crate::damage::node_snapshot::NodeSnapshot)):
    /// this is node-indexed, that is keyed by
    /// [`WidgetId`](crate::primitives::identity::widget_id::WidgetId), since a
    /// widget outlives its index. Index moves coincide with full rebuilds that
    /// overwrite this column, and sharing would need a hash probe on the
    /// repair path.
    pub(super) arena_hashes: Vec<ContentHash>,
    /// The tree's `TreeFingerprint::paint_counts` the paint arena was laid out for: a refresh
    /// patches the arena in place only while it holds.
    pub(super) paint_counts: ContentHash,
    /// Per-node index of the node's row in
    /// [`Cascade::hits`](crate::cascade::Cascade::hits), if it holds one. Which
    /// nodes hold a row is structural, so the incremental walk rewrites a moved
    /// node's row through this.
    pub(super) hit_rows: Vec<Option<NonMaxU32>>,
    /// Per-node scale of the transform the node's shapes paint under, which a backdrop's blur reach
    /// scales by; kept, as the paint rows are, for a node the incremental walk skips.
    pub(super) shape_scales: Vec<f32>,
    /// The plane's backdrops in paint order, rebuilt after every walk from the tree's backdrops
    /// and the rows above.
    pub(crate) backdrops: Vec<PlaneBackdrop>,
    /// The rows of [`Self::backdrops`], sorted, for a search by row: the backdrops are in paint
    /// order, where an ancestor's shape painted after its children has a lower row than theirs.
    pub(crate) backdrop_rows: Vec<u32>,
    /// Offset of this plane's first `EntryRow` in
    /// [`Cascade::entries`](crate::cascade::Cascade::entries), fixed for the
    /// plane's run and set at `reset_for`. A full rebuild pushes one entry per
    /// node; incremental runs rewrite the block in place, so the entry index is
    /// `entries_base + node.0`. With the per-pass
    /// [`Cascade::by_id`](crate::cascade::Cascade::by_id) snapshot this gives
    /// O(1) `WidgetId → entry` without a per-widget hashmap fill.
    pub(crate) entries_base: u32,
}

impl PlaneCascade {
    /// Reset all per-node columns for `node_count` and stamp `entries_base` in one
    /// call, so a caller cannot reset but forget the offset. Per-node columns
    /// are resized and overwritten in place during the walk, retaining
    /// allocation and initialized slots at a stable tree size; `paint_arena`
    /// resets by its own sizing rules.
    pub(super) fn reset_for(&mut self, node_count: usize, entries_base: u32) {
        self.cascade_inputs
            .resize(node_count, CascadeInputHash::default());
        self.subtree_paint_rects.resize(node_count, Rect::ZERO);
        self.subtree_ends.resize(node_count, 0);
        self.paint_arena.reset_for(node_count);
        self.arena_hashes.resize(node_count, ContentHash::default());
        self.hit_rows.resize(node_count, None);
        self.shape_scales.resize(node_count, 1.0);
        self.entries_base = entries_base;
    }

    /// Rebuild [`Self::backdrops`] from the backdrops `tree` recorded, laid out as `layout`, each
    /// at its paint row and that row's screen at `display_scale`, and [`Self::backdrop_rows`] from
    /// them.
    pub(super) fn list_backdrops(&mut self, tree: &Tree, layout: &PlaneLayout, display_scale: f32) {
        self.backdrops.clear();
        let widget_ids = tree.records.widget_id();
        for entry in &tree.backdrops {
            let node = entry.node.idx();
            let span = self.paint_arena.node_spans[node];
            if span.len == 0 {
                continue;
            }
            debug_assert!(
                entry.row < span.len,
                "a backdrop's row lies in its owner's span"
            );
            let row = span.start + entry.row;
            let rect = self.paint_arena.rows[row as usize].screen;
            if rect.is_paint_empty() {
                continue;
            }
            let paint_row = after_children(tree, self, entry).max(row);
            let capture = tree.shapes.records[entry.shape_idx as usize]
                .backdrop_capture()
                .expect("a listed backdrop reads under it");
            let shape_scale = self.shape_scales[node];
            let scale = shape_scale * display_scale;
            let plan = BlurPlan::new(capture.sigma * scale);
            let narrowest =
                narrowest_side(tree, layout, entry, rect, shape_scale).map(|n| n * display_scale);
            self.backdrops.push(PlaneBackdrop {
                key: BackdropKey {
                    owner: widget_ids[node],
                    shape: entry.row,
                },
                row,
                paint_row,
                rect,
                plan,
                footprint: rect.inflated(
                    PlaneBackdrop::reach(plan, capture.reach.margin(scale, narrowest))
                        / display_scale,
                ),
            });
        }
        self.backdrop_rows.clear();
        self.backdrop_rows
            .extend(self.backdrops.iter().map(|backdrop| backdrop.row));
        self.backdrop_rows.sort_unstable();
    }
}

/// The narrowest side, in logical px, of `tree`'s backdrop `entry` drawn whole on the screen,
/// laid out as `layout`, which a lens reads inside of: a shape's box that no paint animation turns
/// and no clip cuts. `rect` is the shape's paint rect, its drop shadow's halo in it, which the
/// test of a clip compares with the same bound lifted; the side is the box's own, as a shadow is
/// no part of the backdrop. `None` for a turning or clipped backdrop, whose screen rect is not its
/// own; and for a triangle, as the lens bounds its reads by a rounded rect's chords, no shorter
/// along an inward normal than its narrowest side (`LensOptics::margin`), which a triangle's
/// corner breaks.
fn narrowest_side(
    tree: &Tree,
    layout: &PlaneLayout,
    entry: &BackdropEntry,
    rect: Rect,
    shape_scale: f32,
) -> Option<f32> {
    {
        if tree.paint_animations.rotates(entry.shape_idx) {
            return None;
        }
        let ShapeRecord::Closed {
            geometry: geometry @ ClosedGeometry::Box(_),
            paint: ClosedPaint::Fill(paint),
        } = &tree.shapes.records[entry.shape_idx as usize]
        else {
            return None;
        };
        let owner_size = layout.rect[entry.node.idx()].size;
        let painted = paint.bbox_local(*geometry, owner_size).size;
        // The lift and a clip that misses the backdrop round the size within a few f32 ulps; a clip
        // that cuts less than this lets a sample out by less than the pixel `PlaneBackdrop::reach`
        // rounds out.
        let whole = |screen: f32, lifted: f32| screen >= lifted * (1.0 - 1e-5);
        if !(whole(rect.size.w, painted.w * shape_scale)
            && whole(rect.size.h, painted.h * shape_scale))
        {
            return None;
        }
        let own = geometry.local_bounds(owner_size).size;
        Some(own.w.min(own.h) * shape_scale)
    }
}

/// The first row past the subtree of the last child `entry`'s node records before the backdrop, `0`
/// for a backdrop no child precedes: a node's rows lie before its children's, each subtree's rows
/// in one run from its root's span, so the run ends where the next node's span starts.
fn after_children(tree: &Tree, cascade: &PlaneCascade, entry: &BackdropEntry) -> u32 {
    let mut child = None;
    for item in TreeItems::new(&tree.records, &tree.shapes.records, entry.node) {
        match item {
            TreeItem::ShapeRecord(idx, _) if idx == entry.shape_idx => break,
            TreeItem::ShapeRecord(..) => {}
            TreeItem::Child(c) => child = Some(c.id),
        }
    }
    let Some(child) = child else {
        return 0;
    };
    let spans = &cascade.paint_arena.node_spans;
    let next = cascade.subtree_ends[child.idx()] as usize;
    let end = spans
        .get(next)
        .map_or(cascade.paint_arena.rows.len() as u32, |span| span.start);
    debug_assert!(
        spans[child.idx()..next]
            .iter()
            .all(|span| span.start >= spans[child.idx()].start && span.start + span.len <= end),
        "a subtree's rows lie in one run, before the next node's"
    );
    end
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::cascade::internals::OwnedPaint;
    use crate::cascade::plane_cascade::PlaneCascade;
    use crate::scene::tree::Tree;
    use crate::scene::tree::iter::TreeItem;
    use crate::scene::tree::node_id::NodeId;

    impl PlaneCascade {
        /// Append `node`'s painting rows and its descendants' with their widgets, in encoder draw order.
        pub(crate) fn owned_paints_of(&self, tree: &Tree, node: NodeId, out: &mut Vec<OwnedPaint>) {
            let arena = &self.paint_arena;
            let span = arena.node_spans[node.idx()];
            // An invisible node's span is empty, and so is every one under it.
            if span.len == 0 {
                return;
            }
            let owner = tree.records.widget_id()[node.idx()];
            let mut row = span.start as usize;
            let push = |row: usize, out: &mut Vec<OwnedPaint>| {
                let paint = arena.rows[row];
                out.push(OwnedPaint {
                    owner,
                    screen: paint.screen,
                    hash: paint.hash,
                    rank: out.len() as u32,
                });
            };
            if tree.chrome(node).is_some() {
                push(row, out);
                row += 1;
            }
            for item in tree.tree_items(node) {
                match item {
                    TreeItem::ShapeRecord(..) => push(row, out),
                    TreeItem::Child(child) => self.owned_paints_of(tree, child.id, out),
                }
                row += 1;
            }
            debug_assert_eq!(row, span.range().end, "rows out of sync with the span");
        }
    }
}
