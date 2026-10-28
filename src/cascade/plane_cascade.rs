//! One plane's cascade columns.

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::paint::PaintArena;
use crate::cascade::pane_row::PaneRow;
use crate::common::content_hash::ContentHash;
use crate::common::non_max_u32::NonMaxU32;
use crate::primitives::geometry::rect::Rect;

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
/// - [`Self::paint_rects`] and [`Self::hit_rows`] are read only by the
///   incremental walk, to refresh a node in place.
#[derive(Debug, Default)]
pub(crate) struct PlaneCascade {
    /// Per-node `cascade_input` fingerprint, indexed like `Tree::records`:
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
    /// rollup or its `cascade_input` moved, and re-stamps it.
    ///
    /// Not mergeable with the damage engine's snapshot of the same rollup
    /// ([`NodeSnapshot::subtree_hash`](crate::damage::node_snapshot::NodeSnapshot)):
    /// this is node-indexed, that is keyed by
    /// [`WidgetId`](crate::primitives::identity::widget_id::WidgetId), since a
    /// widget outlives its index. Index moves coincide with full rebuilds that
    /// overwrite this column, and sharing would need a hash probe on the
    /// repair path.
    pub(super) arena_hashes: Vec<ContentHash>,
    /// Per-node own paint extent, the seed [`Self::subtree_paint_rects`] rolls
    /// up from; the incremental walk rebuilds a node's rollup from it without
    /// recomputing rows.
    pub(super) paint_rects: Vec<Rect>,
    /// Per-node index of the node's row in
    /// [`Cascade::hits`](crate::cascade::Cascade::hits), if it holds one. Which
    /// nodes hold a row is structural, so the incremental walk rewrites a moved
    /// node's row through this.
    pub(super) hit_rows: Vec<Option<NonMaxU32>>,
    /// Per-node scale of the transform the node's shapes paint under, which a pane's blur reach
    /// scales by; kept, as the paint rows are, for a node the incremental walk skips.
    pub(super) shape_scales: Vec<f32>,
    /// The plane's backdrop panes in paint order, rebuilt after every walk from the tree's panes
    /// and the rows above.
    pub(crate) panes: Vec<PaneRow>,
    /// Offset of this plane's first `EntryRow` in
    /// [`Cascade::entries`](crate::cascade::Cascade::entries), fixed for the
    /// plane's run and set at `reset_for`. A full rebuild pushes one entry per
    /// node; incremental runs rewrite the block in place, so the entry index is
    /// `entries_base + node.0`. With the per-pass
    /// [`Cascade::by_id`](crate::cascade::Cascade::by_id) snapshot this gives
    /// O(1) `WidgetId → entry` without a per-widget hashmap fill.
    pub(crate) entries_base: u32,
    /// The plane's `PlaneLayout::rect_hash` these columns were last walked against: while it
    /// holds, the incremental walk skips subtrees whose inputs held.
    pub(super) rects: ContentHash,
}

impl PlaneCascade {
    /// Reset all per-node columns for `n_nodes` and stamp `entries_base` in one
    /// call, so a caller cannot reset but forget the offset. Per-node columns
    /// are resized and overwritten in place during the walk, retaining
    /// allocation and initialized slots at a stable tree size; `paint_arena`
    /// resets by its own sizing rules.
    pub(super) fn reset_for(&mut self, n_nodes: usize, entries_base: u32) {
        self.cascade_inputs
            .resize(n_nodes, CascadeInputHash::default());
        self.subtree_paint_rects.resize(n_nodes, Rect::ZERO);
        self.subtree_ends.resize(n_nodes, 0);
        self.paint_arena.reset_for(n_nodes);
        self.arena_hashes.resize(n_nodes, ContentHash::default());
        self.paint_rects.resize(n_nodes, Rect::ZERO);
        self.hit_rows.resize(n_nodes, None);
        self.shape_scales.resize(n_nodes, 1.0);
        self.entries_base = entries_base;
    }
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
