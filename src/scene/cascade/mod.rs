//! Per-frame post-arrange state.
//!
//! [`CascadeEngine`](engine::CascadeEngine) owns the walk scratch and
//! updates a retained [`Cascade`]. Paint-only changes repair dirty
//! subtrees in place; geometry or inherited-state changes rebuild all
//! per-tree rows. Downstream phases (damage diff, input hit-test,
//! renderer encoder) take `&Cascade` as their single frozen-state
//! handle.
//!
//! Laid out like `layout`: this root holds the retained product, with
//! the machinery in [`engine`], the row tables in [`entry`], and the
//! paint arena in [`paint`].

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod counters;
pub(crate) mod engine;
pub(crate) mod entry;
pub(crate) mod paint;
mod paint_rect;

use crate::common::content_hash::ContentHash;
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::cascade::entry::{EntryRow, HitRow, HitTargets, ScopeRow, WidgetLocation};
use crate::scene::cascade::paint::PaintArena;
use crate::scene::layer::PerLayer;
use crate::scene::seen_ids::Endpoint;
use glam::Vec2;

/// Per-node fingerprint of cascade inputs flowing in from ancestors
/// (parent transform/clip/disabled/invisible) plus the node's own
/// arranged rect, packed with the resolved `invisible` bit. Folded
/// into a 64-bit `FxHash` (lower 63 bits) during the cascade walk;
/// the high bit holds the cascade-resolved `invisible` so encoder
/// and damage can read both in one 8-byte load. Compared
/// frame-over-frame by `DamageEngine::compute`: if this matches AND
/// `subtree[i]` matches, the entire subtree's paint state is
/// bit-identical by induction and the per-node diff jumps to
/// `subtree_end[i]`.
///
/// Why packing is sound: the skip predicate also requires
/// `subtree[i]` match, which covers every descendant's `node_hash`
/// (where own visibility lives). If `subtree` matches AND the lower
/// 63 hash bits match, the high `invisible` bit is implied — own
/// visibility is in `node_hash`, parent_invisible is in the hash
/// inputs.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CascadeInputHash(pub(crate) u64);

const INVISIBLE_BIT: u64 = 1u64 << 63;
const HASH_MASK: u64 = !INVISIBLE_BIT;

impl CascadeInputHash {
    /// Combine a raw 64-bit hash output with the cascade-resolved
    /// `invisible` flag. The hash's top bit is masked off before the
    /// flag is shifted into place — 63 bits of entropy is more than
    /// enough for the skip predicate, and branchless avoids the cost
    /// of a per-node conditional move on the hot cascade path.
    #[inline]
    pub(crate) fn pack(hash: u64, invisible: bool) -> Self {
        Self((hash & HASH_MASK) | ((invisible as u64) << 63))
    }

    #[inline]
    pub(crate) fn invisible(self) -> bool {
        self.0 & INVISIBLE_BIT != 0
    }
}

/// All per-layer cascade state grouped on one struct. The `cascade_inputs`,
/// `subtree_paint_rects`, and `paint_arena` columns are produced together
/// by [`CascadeEngine::run_tree`](engine::CascadeEngine::run_tree),
/// retained together between frames, and read together by the damage diff
/// and encoder.
///
/// ## Columnar split
///
/// The per-node data is deliberately divided four ways, driven by
/// who reads what together:
///
/// - [`Self::cascade_inputs`] is the only datum on the per-node hot
///   path: the encoder reads `cascade_input.invisible()` for every
///   node it walks, and damage compares the full u64 on its skip /
///   descend arms. At 8 B/node the encoder's per-frame walk and
///   damage's scan stay cache-dense.
/// - [`Self::subtree_paint_rects`] is read only by the encoder cull.
/// - [`Self::subtree_hashes`] retains the previous walk's paint
///   invalidation state.
/// - [`Self::subtree_ends`] is read only by [`Cascade::is_within`]
///   ancestry lookups — sparse random access, never a walk, so it
///   must not fatten the walked columns.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`](paint::Paint)s plus the `node_spans` index). Read only on damage's
///   per-shape legs (vacant insert, hash mismatch, paint-anim lookup),
///   so it sits behind a `node_spans[i]` indirection that damage's
///   subtree-skip fast path skips entirely.
#[derive(Debug, Default)]
pub(crate) struct LayerCascade {
    /// Paint-excluding authoring hash from the last full rebuild.
    static_hash: ContentHash,
    /// `Tree::rollups.paint_cardinality` as of the last full rebuild.
    /// The incremental walk can only repair paint rows in place, so a
    /// changed row count sends it home empty-handed after it has already
    /// walked part of the tree; comparing this first turns that wasted
    /// half-walk into an immediate full rebuild.
    paint_cardinality: u64,
    /// `LayerLayout::rect_hash` as of the last full rebuild — the arranged
    /// geometry these retained rows were built against.
    /// [`CascadeEngine::can_update`](engine::CascadeEngine::can_update)
    /// compares it to the live layout's hash to decide whether the retained
    /// non-paint columns still describe the current arrangement.
    layout_hash: ContentHash,
    /// Per-node `cascade_input` fingerprint, indexed the same way as
    /// `Tree::records`: `cascade_inputs[node.idx()]`. Packs the
    /// ancestor state + own arranged rect hash with the cascade-resolved
    /// `invisible` bit in the high position (see [`CascadeInputHash`]).
    /// The encoder reads `.invisible()`; damage pairs the full u64 with
    /// `Tree.rollups.subtree[i]` for its subtree-skip fast path.
    pub(crate) cascade_inputs: Vec<CascadeInputHash>,
    /// Per-node subtree paint rect — the node's own paint extent rolled
    /// up with every descendant's `subtree_paint_rects[i]`. Computed
    /// inline in [`CascadeEngine::run_tree`](engine::CascadeEngine::run_tree)
    /// via a stack-frame accumulator. Read by the encoder for the
    /// viewport + damage subtree culls where "may I skip the whole
    /// subtree?" must consider overhanging descendants — Canvas-positioned
    /// children outside the parent's `Fixed` bound, shapes with
    /// negative-margin overhang, etc. Invisible subtrees seed with
    /// `Rect::ZERO` so a long-lived hidden subtree doesn't keep the cull
    /// from firing at ancestors.
    pub(crate) subtree_paint_rects: Vec<Rect>,
    /// Previous authoring hashes used to skip unchanged subtrees.
    /// Dirty ancestors recompute their own paint rows, so no separate
    /// per-node paint hash or own extent is retained.
    subtree_hashes: Vec<ContentHash>,
    /// Per-node pre-order subtree end (`Tree`'s `subtree_end`, grid
    /// flag stripped), snapshotted so ancestry queries
    /// ([`Cascade::is_within`]) can run against the frozen cascade
    /// result *during the next record* — by then the live tree's
    /// columns are already being rebuilt. Indexed like
    /// `cascade_inputs`.
    subtree_ends: Vec<u32>,
    /// Unified paint arena (rows + per-node spans).
    pub(super) paint_arena: PaintArena,
    /// Offset of this layer's first `EntryRow` in
    /// [`Cascade::entries`] — fixed for the layer's run, set at
    /// `reset_for` time. A full rebuild pushes one entry per node;
    /// paint-only runs retain the block. The entry index is therefore
    /// always `entries_base + node.0`. Combined with the per-pass
    /// [`Cascade::by_id`] snapshot this gives O(1) `WidgetId → entry`
    /// without a per-widget `WidgetId → u32` hashmap fill.
    pub(super) entries_base: u32,
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
    fn reset_for(&mut self, n_nodes: usize, entries_base: u32) {
        self.cascade_inputs
            .resize(n_nodes, CascadeInputHash::default());
        self.subtree_paint_rects.resize(n_nodes, Rect::ZERO);
        self.subtree_hashes.resize(n_nodes, ContentHash::default());
        self.subtree_ends.resize(n_nodes, 0);
        self.paint_arena.reset_for(n_nodes);
        self.entries_base = entries_base;
    }
}

/// Read-only artifact of `CascadeEngine::run`. Holds per-layer
/// cascade state (per-node rows, subtree rollups, paint arena — see
/// [`LayerCascade`]) plus the [`Self::by_id`] hit-lookup snapshot.
#[derive(Debug, Default)]
pub(crate) struct Cascade {
    pub(crate) layers: PerLayer<LayerCascade>,
    /// One row per recorded node, node-aligned within each layer's
    /// block (`layers[l].entries_base + node.0`). Read only by
    /// per-widget response lookup, which gathers a whole row at one
    /// index — see [`EntryRow`] for why it is AoS. Layers append in
    /// paint order.
    pub(crate) entries: Vec<EntryRow>,
    /// Interactive rows only, in the same paint order as
    /// [`Self::entries`]. Hit tests reverse-scan this table and read
    /// nothing else — see [`HitRow`].
    pub(crate) hits: Vec<HitRow>,
    /// Declared input scopes in record order — see [`ScopeRow`].
    pub(crate) scopes: Vec<ScopeRow>,
    /// `WidgetId → Endpoint` lookup for hit-test consumers
    /// ([`crate::input::InputState::response_for`], capture / focus
    /// eviction). **Invariant: equals `SeenIds.curr` as observed at
    /// the end of the most recent `CascadeEngine::run`** — a full
    /// rebuild populates it with `clone_from(&seen.curr)`; paint-only
    /// runs retain it because their preflight includes every widget
    /// identity. The snapshot is required (rather than reading
    /// `seen.curr` directly) because `response_for` is called during
    /// recording, and `SeenIds::pre_record` clears `curr` at the top
    /// of every record pass — `request_relayout`'s second pass needs
    /// to see pass A's entries while its own widgets are still being
    /// recorded into the freshly-cleared `curr`. `seen.prev` is the
    /// wrong fallback: it carries the previous *frame*'s data, not
    /// the previous *pass*'s. Pays one O(N) memcpy per cascade run
    /// on a full rebuild in exchange for not paying an O(N) hashmap
    /// insert per widget.
    pub(crate) by_id: WidgetIdMap<Endpoint>,
}

impl Cascade {
    /// Where `id` was recorded in the most recent cascade run — the
    /// `(layer, node)` pair the per-node layout columns are indexed by —
    /// or `None` if it isn't in that run.
    ///
    /// The handle half of a lookup whose other half lives on `Layout`
    /// (see [`Layout::scroll_content`](crate::layout::Layout::scroll_content)):
    /// neither table can answer alone, so a caller joins them, and this
    /// is what keeps the map itself out of the join.
    #[inline]
    pub(crate) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.by_id.get(&id).copied()
    }

    /// Both indexes a widget's per-frame rows are reached by, from one
    /// `by_id` probe. `response_for` needs the entry index (for
    /// [`Cascade::entries`]) *and* the endpoint (for the layout
    /// columns, which are keyed by `(layer, node)`), once per widget per
    /// frame — resolving them separately would double the hash lookups
    /// on that path.
    #[inline]
    pub(crate) fn locate(&self, id: WidgetId) -> Option<WidgetLocation> {
        let endpoint = *self.by_id.get(&id)?;
        Some(WidgetLocation {
            entry_idx: self.layers[endpoint.layer].entries_base + endpoint.node.0,
            endpoint,
        })
    }

    /// True when `descendant`'s most recent record sits inside
    /// `ancestor`'s subtree — same layer, within the ancestor's
    /// pre-order interval `[node, subtree_end)`. Self-inclusive:
    /// `is_within(id, id)` is `true` for any recorded `id`. `false`
    /// when either id wasn't in the most recent cascade run (layers
    /// are separate trees, so a popup is never "within" its anchor).
    pub(crate) fn is_within(&self, descendant: WidgetId, ancestor: WidgetId) -> bool {
        let (Some(d), Some(a)) = (self.by_id.get(&descendant), self.by_id.get(&ancestor)) else {
            return false;
        };
        d.layer == a.layer
            && d.node.0 >= a.node.0
            && d.node.0 < self.layers[a.layer].subtree_ends[a.node.idx()]
    }

    /// Interactive rows under `pos`, topmost first.
    ///
    /// The one reverse scan every hit test is built from: rows are pushed
    /// in paint order, so walking back yields the topmost match first and
    /// the caller stops when it has what it needs.
    #[inline]
    fn hits_under(&self, pos: Vec2) -> impl Iterator<Item = &HitRow> {
        self.hits
            .iter()
            .rev()
            .filter(move |row| row.rect.contains(pos))
    }

    /// Topmost row under `pos` passing `gate`. Shared by [`Self::hit_test`]
    /// and [`Self::hit_test_focusable`], which differ only in the field of
    /// the row they gate on.
    fn hit_first(&self, pos: Vec2, gate: impl Fn(&HitRow) -> bool) -> Option<WidgetId> {
        self.hits_under(pos)
            .find(|row| gate(row))
            .map(|row| row.widget_id)
    }

    /// Topmost entry under `pos` whose `Sense` passes `filter` (hoverable for
    /// hover, clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        self.hit_first(pos, |row| filter(row.sense))
    }

    /// One reverse walk that finds the topmost match for each of three
    /// filters at once. Used on `PointerMoved` and at `post_record` to
    /// recompute hover + scroll + pinch targets in a single pass.
    /// Independent filters: a `Sense::DRAG | Sense::SCROLL` widget sits in
    /// both hover and scroll target slots if it's the topmost match for
    /// each. Stops as soon as all three are filled.
    pub(crate) fn hit_test_targets(
        &self,
        pos: Vec2,
        hover_filter: impl Fn(Sense) -> bool,
        scroll_filter: impl Fn(Sense) -> bool,
        pinch_filter: impl Fn(Sense) -> bool,
    ) -> HitTargets {
        // Three explicit arms rather than a loop over boxed filters: this
        // runs per pointer move, and the closures stay monomorphised.
        let mut targets = HitTargets::default();
        for row in self.hits_under(pos) {
            if targets.hover.is_none() && hover_filter(row.sense) {
                targets.hover = Some(row.widget_id);
            }
            if targets.scroll.is_none() && scroll_filter(row.sense) {
                targets.scroll = Some(row.widget_id);
            }
            if targets.pinch.is_none() && pinch_filter(row.sense) {
                targets.pinch = Some(row.widget_id);
            }
            if targets.hover.is_some() && targets.scroll.is_some() && targets.pinch.is_some() {
                break;
            }
        }
        targets
    }

    pub(crate) fn hit_test_focusable(&self, pos: Vec2) -> Option<WidgetId> {
        self.hit_first(pos, |row| row.focusable)
    }
}

#[cfg(test)]
mod tests;
