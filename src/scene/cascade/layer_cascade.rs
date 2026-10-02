//! One layer's cascade columns.

use crate::common::content_hash::ContentHash;
use crate::primitives::rect::Rect;
use crate::scene::cascade::cascade_input_hash::CascadeInputHash;
use crate::scene::cascade::paint::PaintArena;

/// All per-layer cascade state grouped on one struct. The `cascade_inputs`,
/// `subtree_paint_rects`, and `paint_arena` columns are produced together
/// by [`CascadeEngine::run_tree`](crate::scene::cascade::engine::CascadeEngine::run_tree),
/// retained together between frames, and read together by the damage diff
/// and encoder.
///
/// ## Columnar split
///
/// The per-node data is deliberately divided five ways, driven by
/// who reads what together:
///
/// - [`Self::cascade_inputs`] is the only datum on the per-node hot
///   path: the encoder reads `cascade_input.invisible()` for every
///   node it walks, and damage compares the full u64 on its skip /
///   descend arms. At 8 B/node the encoder's per-frame walk and
///   damage's scan stay cache-dense.
/// - [`Self::subtree_paint_rects`] answers "what does this subtree
///   paint" — the encoder's cull, and damage's two moved-subtree
///   pushes. One column rather than a fold each caller runs over the
///   rows, so the answer cannot depend on who asked.
/// - [`Self::subtree_ends`] is read only by [`Cascade::is_within`](crate::scene::cascade::Cascade::is_within)
///   ancestry lookups — sparse random access, never a walk, so it
///   must not fatten the walked columns.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`](crate::scene::cascade::paint::Paint)s plus the `node_spans` index). Read only on damage's
///   per-shape legs (vacant insert, hash mismatch, paint-anim lookup),
///   so it sits behind a `node_spans[i]` indirection that damage's
///   subtree-skip fast path skips entirely.
/// - [`Self::arena_hashes`] stamps the retained `paint_arena` rows with
///   the authoring rollup they were built from — provenance, not a
///   walked column.
#[derive(Debug, Default)]
pub(crate) struct LayerCascade {
    /// Per-node `cascade_input` fingerprint, indexed the same way as
    /// `Tree::records`: `cascade_inputs[node.idx()]`. Packs the
    /// ancestor state + own arranged rect hash with the cascade-resolved
    /// `invisible` bit in the high position (see [`CascadeInputHash`]).
    /// The encoder reads `.invisible()`; damage pairs the full u64 with
    /// `Tree.rollups.subtree[i]` for its subtree-skip fast path.
    pub(crate) cascade_inputs: Vec<CascadeInputHash>,
    /// Per-node subtree paint rect — the node's own paint extent rolled
    /// up with every descendant's `subtree_paint_rects[i]`. Computed
    /// inline in [`CascadeEngine::run_tree`](crate::scene::cascade::engine::CascadeEngine::run_tree)
    /// via a stack-frame accumulator.
    ///
    /// Read by the encoder for the viewport + damage subtree culls where
    /// "may I skip the whole subtree?" must consider overhanging
    /// descendants — Canvas-positioned children outside the parent's
    /// `Fixed` bound, shapes with negative-margin overhang, etc. Damage
    /// reads the same column wherever it asks the same question: the
    /// extent a moved subtree paints, and a child marker's extent in the
    /// order-inversion check.
    ///
    /// Invisible subtrees seed with `Rect::ZERO` so a long-lived hidden
    /// subtree doesn't keep the cull from firing at ancestors — and so a
    /// hidden subtree that moves damages nothing, where a fold over its
    /// rows would repaint pixels no pass paints. A clip-only container's
    /// own visible rect is in here and in no row, which only ever makes
    /// the answer cover more than the subtree's rows do.
    pub(crate) subtree_paint_rects: Vec<Rect>,
    /// Per-node pre-order subtree end (`Tree`'s `subtree_end`, grid
    /// flag stripped), snapshotted so ancestry queries
    /// ([`Cascade::is_within`](crate::scene::cascade::Cascade::is_within)) can run against the frozen cascade
    /// result *during the next record* — by then the live tree's
    /// columns are already being rebuilt. Indexed like
    /// `cascade_inputs`.
    pub(super) subtree_ends: Vec<u32>,
    /// Unified paint arena (rows + per-node spans).
    pub(crate) paint_arena: PaintArena,
    /// Per-node `Tree.rollups.subtree` the retained [`Self::paint_arena`]
    /// rows were built from — the per-node half of the validity gate
    /// whose whole-layer half is [`Cascade::key`](crate::scene::cascade::Cascade::key). An
    /// incremental repair descends exactly where this disagrees with the
    /// live rollup and re-stamps what it repaired. Dirty ancestors
    /// recompute their own paint rows, so no separate per-node paint hash
    /// or own extent is retained.
    ///
    /// **Not the damage engine's snapshot of the same rollup.** The two
    /// hold equal values and cannot be merged: this one is node-indexed,
    /// and [`NodeSnapshot::subtree_hash`](crate::scene::damage::node_snapshot::NodeSnapshot)
    /// is keyed by [`WidgetId`](crate::primitives::widget_id::WidgetId) because a widget outlives the index it
    /// occupied. The frames where a widget's index moves are exactly the
    /// frames a full rebuild overwrites this whole column, so neither
    /// reader can answer from the other's copy without a per-node hash
    /// probe on the repair path.
    pub(super) arena_hashes: Vec<ContentHash>,
    /// Offset of this layer's first `EntryRow` in
    /// [`Cascade::entries`](crate::scene::cascade::Cascade::entries) — fixed for the layer's run, set at
    /// `reset_for` time. A full rebuild pushes one entry per node;
    /// paint-only runs retain the block. The entry index is therefore
    /// always `entries_base + node.0`. Combined with the per-pass
    /// [`Cascade::by_id`](crate::scene::cascade::Cascade::by_id) snapshot this gives O(1) `WidgetId → entry`
    /// without a per-widget `WidgetId → u32` hashmap fill.
    pub(crate) entries_base: u32,
}

impl LayerCascade {
    /// Reset all per-node columns for `n_nodes` and stamp the layer's
    /// `entries_base` in one call — both prep this
    /// layer for the upcoming `run_tree`, splitting them invites a
    /// caller that resets but forgets the offset (or vice versa).
    /// The fixed-size per-node columns are resized once and overwritten
    /// in place during the walk, retaining both allocation and initialized
    /// slots when the tree size is stable;
    /// `paint_arena` columns reset according to their own sizing rules.
    pub(super) fn reset_for(&mut self, n_nodes: usize, entries_base: u32) {
        self.cascade_inputs
            .resize(n_nodes, CascadeInputHash::default());
        self.subtree_paint_rects.resize(n_nodes, Rect::ZERO);
        self.subtree_ends.resize(n_nodes, 0);
        self.paint_arena.reset_for(n_nodes);
        self.arena_hashes.resize(n_nodes, ContentHash::default());
        self.entries_base = entries_base;
    }
}
