//! Per-frame post-arrange state.
//!
//! `CascadesEngine` owns the walk scratch and updates a retained
//! `Cascades`. Paint-only changes repair dirty subtrees in place;
//! geometry or inherited-state changes rebuild all per-tree rows.
//! Downstream phases (damage diff, input hit-test, renderer encoder)
//! take `&Cascades` as their single frozen-state handle.

use crate::common::hash::Hasher;
use crate::display::Display;
use crate::forest::Forest;

use crate::common::content_hash::ContentHash;
use crate::forest::layer::PerLayer;
use crate::forest::seen_ids::Endpoint;
use crate::forest::shapes::record::{ShapeRecord, shadow_paint_rect_local, text_paint_bbox_local};
use crate::forest::tree::Tree;
use crate::forest::tree::iter::{TreeItem, TreeItems};
use crate::forest::tree::node::NodeId;
use crate::forest::tree::recording::Placement;
use crate::input::sense::Sense;
use crate::layout::scroll::ScrollStates;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::approx;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::widget_id::WidgetIdMap;
use crate::primitives::{rect::Rect, transform::TranslateScale};
use crate::renderer::render_buffer::curve::{HALF_FRINGE, stroked_bbox};
use crate::text::TEXT_SCALE_STEP;
use glam::Vec2;
use soa_rs::{Soa, Soars};
use std::hash::Hasher as _;

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

/// One row of a node's paint span — chrome (row 0 when the node has
/// chrome), one direct shape, or a child marker, in record order.
/// Single source of truth for "did this pixel-producer change since
/// last frame?" — including paint *order*: child markers put the
/// shape/child interleave into the span, so the damage diff's row
/// matcher sees z-order changes (a raised node, a shape crossing a
/// child boundary) as row reorders, not silent no-ops.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    /// Screen-space rect after parent transform + clip. Child markers
    /// carry `Rect::ZERO` — they produce no pixels themselves (the
    /// child's own rows do); damage computes a child's painted extent
    /// on demand from its subtree's rows when an order check needs it.
    pub(crate) screen: Rect,
    /// Authoring hash. For chrome: `Tree.rollups.chrome[node]`.
    /// For shape: `Tree.shapes.hashes[shape_idx]`. For a child marker:
    /// the child's `WidgetId` bits — its stable identity across
    /// reorders.
    pub(crate) hash: ContentHash,
}

/// Per-layer paint state: the unified [`Paint`] arena plus a per-node
/// index into it. A full cascade rebuild resets it; an incremental
/// pass copies changed spans into retained rows.
#[derive(Debug, Default)]
pub(crate) struct PaintArena {
    /// One [`Paint`] row per chrome contribution (row 0 of a node's
    /// span when present), direct shape, or immediate-child marker,
    /// in record order per node. Pushed in pre-order paint order;
    /// cleared by [`Self::reset_for`].
    pub(crate) rows: Vec<Paint>,
    /// Per-node [`Span`] into [`Self::rows`]. Empty span
    /// (`Span::default()`) means the node paints nothing — replaces
    /// the old `rollups.paints` bitset.
    pub(crate) node_spans: Vec<Span>,
}

impl PaintArena {
    /// Reset both columns for a new frame. `n_nodes` resizes
    /// `node_spans`; every retained slot is overwritten by
    /// [`compute_paint_rect`]. `rows` is cleared and reserved for the
    /// expected upper bound.
    pub(crate) fn reset_for(&mut self, n_nodes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.resize(n_nodes, Span::default());
    }
}

/// One per-node cascade row. Stored as `Soa<EntryRow>` on
/// [`Cascades::entries`] so each field becomes its own contiguous
/// slice. Hit tests use [`Cascades::hits`] to visit only rows
/// that can interact, reading `rect` and the relevant flags while
/// response lookup reaches every row through [`Cascades::by_id`].
/// Same cache argument as aperture's `Tree.records: Soa<NodeRecord>`.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct EntryRow {
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    /// Read for rows referenced by [`Cascades::hits`].
    pub rect: Rect,
    /// Pointer interactions this row participates in (`HOVER` / `CLICK`
    /// / `DRAG` / `SCROLL`).
    pub sense: Sense,
    /// Focus eligibility — checked by the focusable hit-test only.
    pub focusable: bool,
    /// Effective disabled (self OR any ancestor). Mirrors what
    /// `cascaded_off` already used to null `sense`/`focusable`,
    /// preserved here so per-widget responses can read it.
    pub disabled: bool,
    /// Pre-transform layout rect (unclipped, in world coords).
    /// Surfaced via `ResponseState::layout_rect` so callers can read
    /// a widget's arranged position without the cascade's transform +
    /// clip applied — useful for drawing connection geometry into a
    /// scrolling/zoomed parent's coordinate system.
    pub layout_rect: Rect,
    /// The cumulative ancestor transform mapping this node's `layout_rect`
    /// into unclipped surface space. The visible `rect` may be smaller
    /// after ancestor clipping. Surfaced via `ResponseState::transform`
    /// for converting surface-space vectors into widget-local logical
    /// coordinates — `IDENTITY` when untransformed.
    pub transform: TranslateScale,
}

#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct HitRow {
    pub entry_idx: u32,
    pub widget_id: WidgetId,
}

struct Frame {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    subtree_end: u32,
    /// Node index this frame represents — used to write back
    /// `subtree_paint_rect` into `Cascades::subtree_paint_rects` when
    /// this frame is popped (its subtree has been fully visited).
    node_idx: usize,
    /// Running union of this node's own `paint_rect` and the
    /// `subtree_paint_rect` of every descendant whose subtree has
    /// already been folded in. Each pop unions this into the new
    /// top frame so the rollup ripples upward to the root.
    subtree_paint_rect: Rect,
    /// FxHasher state pre-populated with this frame's ancestor-derived
    /// hash inputs (transform / clip / disabled / invisible). Cloned
    /// once per descendant to seed `cascade_input` — descendants only
    /// fold in their own `layout_rect`, avoiding a re-hash of the 32 B
    /// ancestor prefix per node. See `finish_cascade_input`.
    cascade_prefix: Hasher,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("transform", &self.transform)
            .field("clip", &self.clip)
            .field("disabled", &self.disabled)
            .field("invisible", &self.invisible)
            .field("subtree_end", &self.subtree_end)
            .field("node_idx", &self.node_idx)
            .field("subtree_paint_rect", &self.subtree_paint_rect)
            .finish_non_exhaustive()
    }
}

/// All per-layer cascade state grouped on one struct. The
/// `cascade_inputs`, `subtree_paint_rects`, and `paint_arena` columns
/// are produced together by [`run_tree`], retained together between
/// frames, and read together by the damage diff and encoder.
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
/// - [`Self::subtree_ends`] is read only by [`Cascades::is_within`]
///   ancestry lookups — sparse random access, never a walk, so it
///   must not fatten the walked columns.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`]s plus the `node_spans` index). Read only on damage's
///   per-shape legs (vacant insert, hash mismatch, paint-anim lookup),
///   so it sits behind a `node_spans[i]` indirection that damage's
///   subtree-skip fast path skips entirely.
#[derive(Debug, Default)]
pub(crate) struct LayerCascades {
    /// Paint-excluding authoring hash from the last full rebuild.
    static_hash: ContentHash,
    /// Per-node `cascade_input` fingerprint, indexed the same way as
    /// `Tree::records`: `cascade_inputs[node.idx()]`. Packs the
    /// ancestor state + own arranged rect hash with the cascade-resolved
    /// `invisible` bit in the high position (see [`CascadeInputHash`]).
    /// The encoder reads `.invisible()`; damage pairs the full u64 with
    /// `Tree.rollups.subtree[i]` for its subtree-skip fast path.
    pub(crate) cascade_inputs: Vec<CascadeInputHash>,
    /// Per-node subtree paint rect — the node's own paint extent rolled
    /// up with every descendant's `subtree_paint_rects[i]`. Computed
    /// inline in [`run_tree`] via a stack-frame accumulator. Read by
    /// the encoder for the viewport + damage subtree culls where
    /// "may I skip the whole subtree?" must consider overhanging
    /// descendants — Canvas-positioned children outside the parent's
    /// `Fixed` bound, shapes with negative-margin overhang, etc.
    /// Invisible subtrees seed with `Rect::ZERO` so a long-lived
    /// hidden subtree doesn't keep the cull from firing at ancestors.
    pub(crate) subtree_paint_rects: Vec<Rect>,
    /// Previous authoring hashes used to skip unchanged subtrees.
    /// Dirty ancestors recompute their own paint rows, so no separate
    /// per-node paint hash or own extent is retained.
    subtree_hashes: Vec<ContentHash>,
    /// Per-node pre-order subtree end (`Tree`'s `subtree_end`, grid
    /// flag stripped), snapshotted so ancestry queries
    /// ([`Cascades::is_within`]) can run against the frozen cascade
    /// result *during the next record* — by then the live tree's
    /// columns are already being rebuilt. Indexed like
    /// `cascade_inputs`.
    pub(crate) subtree_ends: Vec<u32>,
    /// Unified paint arena (rows + per-node spans).
    pub(crate) paint_arena: PaintArena,
    /// Offset of this layer's first `EntryRow` in
    /// [`Cascades::entries`] — fixed for the layer's run, set at
    /// `reset_for` time. A full rebuild pushes one entry per node;
    /// paint-only runs retain the block. The entry index is therefore
    /// always `entries_base + node.0`. Combined with the per-pass
    /// [`Cascades::by_id`] snapshot this gives O(1) `WidgetId → entry`
    /// without a per-widget `WidgetId → u32` hashmap fill.
    pub(crate) entries_base: u32,
}

impl LayerCascades {
    /// Reset all per-node columns for `n_nodes` and stamp the layer's
    /// `entries_base` in one call — both prep this
    /// layer for the upcoming `run_tree`, splitting them invites a
    /// caller that resets but forgets the offset (or vice versa).
    /// The fixed-size per-node columns are resized once and overwritten
    /// in place during the walk, retaining both allocation and initialized
    /// slots when the tree size is stable;
    /// `paint_arena` columns reset according to their own sizing rules.
    pub(crate) fn reset_for(&mut self, n_nodes: usize, entries_base: u32) {
        self.cascade_inputs
            .resize(n_nodes, CascadeInputHash::default());
        self.subtree_paint_rects.resize(n_nodes, Rect::ZERO);
        self.subtree_hashes.resize(n_nodes, ContentHash::default());
        self.subtree_ends.resize(n_nodes, 0);
        self.paint_arena.reset_for(n_nodes);
        self.entries_base = entries_base;
    }
}

/// Read-only artifact of `CascadesEngine::run`. Holds per-layer
/// cascade state (per-node rows, subtree rollups, paint arena — see
/// [`LayerCascades`]) plus the [`Self::by_id`] hit-lookup snapshot.
#[derive(Debug, Default)]
pub(crate) struct Cascades {
    pub(crate) layers: PerLayer<LayerCascades>,
    /// Pre-order hit-test rows in SoA form — each field is its own
    /// contiguous slice (`entries.rect()`, `entries.sense()`,
    /// `entries.layout_rect()`, …), keeping response lookups
    /// node-aligned while hit tests reach interactive rows through
    /// [`Self::hits`].
    /// Layers append in paint order so reverse iteration yields
    /// topmost-first.
    pub(crate) entries: Soa<EntryRow>,
    /// Entry indices and widget IDs for rows whose effective `sense`
    /// is nonempty or which are focusable, in the same paint order as
    /// [`Self::entries`]. Hit tests reverse-scan this compact table while
    /// response lookup retains the full node-aligned entry table. SoA
    /// keeps the two columns at 12 bytes per hit while one row push
    /// updates both; `Vec<HitRow>` would pad each row to 16 bytes.
    pub(crate) hits: Soa<HitRow>,
    /// `WidgetId → Endpoint` lookup for hit-test consumers
    /// ([`crate::input::InputState::response_for`], capture / focus
    /// eviction). **Invariant: equals `SeenIds.curr` as observed at
    /// the end of the most recent `CascadesEngine::run`** — a full
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

impl Cascades {
    /// Global entry index of the widget last recorded under `id`,
    /// or `None` if `id` isn't in the most recent cascade run.
    #[inline]
    pub(crate) fn entry_idx_of(&self, id: WidgetId) -> Option<u32> {
        let ep = self.by_id.get(&id)?;
        Some(self.layers[ep.layer].entries_base + ep.node.0)
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

    fn hit_rows(&self) -> impl DoubleEndedIterator<Item = (usize, WidgetId)> + '_ {
        self.hits
            .entry_idx()
            .iter()
            .zip(self.hits.widget_id())
            .map(|(&entry, &widget_id)| (entry as usize, widget_id))
    }

    /// Reverse walk (topmost-first under the pre-order paint walk) returning
    /// the first entry whose rect contains `pos` and whose `gate(i)` passes.
    /// Shared by [`Self::hit_test`] and [`Self::hit_test_focusable`], which
    /// differ only in the per-entry gate column they consult.
    fn hit_first(&self, pos: Vec2, gate: impl Fn(usize) -> bool) -> Option<WidgetId> {
        let rects = self.entries.rect();
        for (i, widget_id) in self.hit_rows().rev() {
            if gate(i) && rects[i].contains(pos) {
                return Some(widget_id);
            }
        }
        None
    }

    /// Topmost entry under `pos` whose `Sense` passes `filter` (hoverable for
    /// hover, clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        let senses = self.entries.sense();
        self.hit_first(pos, |i| filter(senses[i]))
    }

    /// One reverse walk that finds the topmost match for each of
    /// three filters at once. Used on `PointerMoved` and at
    /// `post_record` to recompute hover + scroll + pinch targets in a
    /// single pass over `entries`. Independent filters: a `Sense::DRAG
    /// | Sense::SCROLL` widget sits in both hover and scroll target
    /// slots if it's the topmost match for each.
    pub(crate) fn hit_test_targets(
        &self,
        pos: Vec2,
        hover_filter: impl Fn(Sense) -> bool,
        scroll_filter: impl Fn(Sense) -> bool,
        pinch_filter: impl Fn(Sense) -> bool,
    ) -> HitTargets {
        let rects = self.entries.rect();
        let senses = self.entries.sense();
        let mut hover = None;
        let mut scroll = None;
        let mut pinch = None;
        for (i, widget_id) in self.hit_rows().rev() {
            if !rects[i].contains(pos) {
                continue;
            }
            if hover.is_none() && hover_filter(senses[i]) {
                hover = Some(widget_id);
            }
            if scroll.is_none() && scroll_filter(senses[i]) {
                scroll = Some(widget_id);
            }
            if pinch.is_none() && pinch_filter(senses[i]) {
                pinch = Some(widget_id);
            }
            if hover.is_some() && scroll.is_some() && pinch.is_some() {
                break;
            }
        }
        HitTargets {
            hover,
            scroll,
            pinch,
        }
    }

    pub(crate) fn hit_test_focusable(&self, pos: Vec2) -> Option<WidgetId> {
        let focusables = self.entries.focusable();
        self.hit_first(pos, |i| focusables[i])
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitTargets {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: Option<WidgetId>,
    pub(crate) pinch: Option<WidgetId>,
}

#[derive(Debug, Default)]
pub(crate) struct CascadesEngine {
    stack: Vec<Frame>,
    paint_scratch: PaintArena,
    display_scale: Option<f32>,
}

impl CascadesEngine {
    /// Update the frozen cascade result. Stable subtrees are retained
    /// in place; a paint-row cardinality or tree-size change falls
    /// back to a complete rebuild.
    #[profiling::function]
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascades: &mut Cascades,
    ) {
        if !self.can_update(forest, layout, display, cascades) {
            self.run_full(forest, layout, display, cascades);
            return;
        }

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            self.stack.clear();
            self.paint_scratch.reset_for(n);
            let incremental_complete = self.run_tree::<true>(
                tree,
                &layout.layers[layer],
                &mut cascades.layers[layer],
                &mut cascades.entries,
                &mut cascades.hits,
                display.scale_factor,
            );
            if !incremental_complete {
                self.run_full(forest, layout, display, cascades);
                return;
            }
        }
    }

    /// A match proves every retained non-paint cascade and hit-test
    /// column remains valid; the incremental walk only repairs paint.
    fn can_update(
        &self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascades: &Cascades,
    ) -> bool {
        if self.display_scale != Some(display.scale_factor) {
            return false;
        }
        let total: usize = forest.trees.0.iter().map(|tree| tree.records.len()).sum();
        if cascades.entries.len() != total {
            return false;
        }
        let mut entries_base = 0u32;
        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let lc = &cascades.layers[layer];
            if lc.entries_base != entries_base
                || lc.static_hash != tree.rollups.cascade_static
                || lc.subtree_hashes.len() != n
            {
                return false;
            }
            let base = entries_base as usize;
            if cascades.entries.layout_rect()[base..base + n] != layout.layers[layer].rect {
                return false;
            }
            if lc
                .subtree_ends
                .iter()
                .zip(tree.records.subtree_end())
                .any(|(&previous, current)| previous != current.end())
            {
                return false;
            }
            entries_base += n as u32;
        }
        true
    }

    pub(crate) fn run_full(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascades: &mut Cascades,
    ) {
        let total: usize = forest.trees.0.iter().map(|tree| tree.records.len()).sum();
        cascades.entries.clear();
        cascades.entries.reserve(total);
        cascades.hits.clear();

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let entries_base = cascades.entries.len() as u32;
            cascades.layers[layer].reset_for(n, entries_base);
            self.stack.clear();
            let full_complete = self.run_tree::<false>(
                tree,
                &layout.layers[layer],
                &mut cascades.layers[layer],
                &mut cascades.entries,
                &mut cascades.hits,
                display.scale_factor,
            );
            assert!(full_complete);
            cascades.layers[layer]
                .subtree_hashes
                .copy_from_slice(&tree.rollups.subtree);
            assert_eq!(
                cascades.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree must emit one entry per recorded node",
            );
            cascades.layers[layer].static_hash = tree.rollups.cascade_static;
        }

        // `SeenIds::pre_record` clears `curr` before a relayout pass can
        // query the preceding pass's responses.
        cascades.by_id.clone_from(&forest.ids.curr);
        self.display_scale = Some(display.scale_factor);
    }
}

/// Fingerprint of everything [`CascadesEngine::run`] reads, cheaply.
/// Equal fingerprints across two frames ⇒ identical cascade output, so
/// `Ui::post_record` skips the run and reuses last frame's `Cascades`
/// (O5 stage 0 — full-frame skip, gated on the frame runtime's cascade fingerprint).
/// Folds:
/// - the exact surface (a sub-quantum resize can hit the measure
///   cache yet still re-arrange, so the *exact* rect must be here);
/// - every root's `subtree_hash`, which already captures all cascade
///   authoring — transforms (`PanelExtras`), clip/disabled/focusable
///   (`attrs`), visibility, shapes, chrome;
/// - scroll `offset`/`zoom`, the one cross-frame arrange input that
///   lives in `LayoutEngine.scroll_states`, not in `subtree_hash`.
///
/// Lives here, beside the walk it mirrors, on purpose: the skip is
/// only sound while this enumeration covers every input `run_tree`
/// (and the arrange pass feeding it) consumes. Adding a cascade input
/// without folding it here silently reuses stale cascades — keep the
/// two in one review's field of view.
pub(crate) fn cascade_fingerprint(
    forest: &Forest,
    scroll_states: &ScrollStates,
    display: Display,
) -> u64 {
    let mut h = Hasher::new();
    h.write_u32(display.physical.x);
    h.write_u32(display.physical.y);
    approx::hash_f32(display.scale_factor, &mut h);
    for (layer, tree) in forest.trees.iter_paint_order() {
        // Layer discriminant: an identical root subtree migrating
        // between side layers (Popup → Tooltip) must not alias, or
        // the skip reuses per-layer columns sized for the old
        // assignment and the damage pass indexes them out of
        // bounds.
        h.write_u8(layer as u8);
        for slot in &tree.roots {
            // A root's own id does not reach the subtree hash used by
            // this fingerprint — `compute_rollups` folds only child ids
            // into parents — so include it directly.
            h.write_u64(tree.records.widget_id()[slot.first_node.idx()].0);
            h.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            // Placement lives outside node hashes but changes arranged rects.
            match slot.placement {
                Placement::Fixed { anchor, size } => {
                    h.write_u8(0);
                    approx::hash_visual_vec2(anchor, &mut h);
                    match size {
                        Some(size) => {
                            h.write_u8(1);
                            approx::hash_visual_size(size, &mut h);
                        }
                        None => h.write_u8(0),
                    }
                }
                Placement::Overlay(position) => {
                    h.write_u8(1);
                    approx::hash_visual_rect(position.anchor, &mut h);
                    h.write_u8(position.side as u8);
                    h.write_u8(position.align as u8);
                    approx::hash_visual_f32(position.gap, &mut h);
                }
            }
        }
    }
    // XOR fold so map iteration order doesn't matter.
    let mut scroll_fold = 0u64;
    for (wid, st) in scroll_states.iter() {
        let mut sh = Hasher::new();
        sh.write_u64(wid.0);
        approx::hash_visual_vec2(st.offset, &mut sh);
        approx::hash_visual_f32(st.zoom - 1.0, &mut sh);
        scroll_fold ^= sh.finish();
    }
    h.finish() ^ scroll_fold
}

/// Finalize one stack frame: write the rolled-up
/// `subtree_paint_rect` into the parallel `subtree_paint_rects` slot
/// for the frame's node, then union upward into the now-top frame so
/// the rollup ripples to the root. Called from both the per-node
/// pop loop and the end-of-tree drain — identical logic, one source.
#[inline]
fn finalize_frame(stack: &mut [Frame], subtree_paint_rects: &mut [Rect], popped: Frame) {
    subtree_paint_rects[popped.node_idx] = popped.subtree_paint_rect;
    if let Some(parent) = stack.last_mut() {
        // A subtree that paints nothing carries the `Rect::ZERO` seed;
        // `union` treats it as identity, so it can't anchor the
        // ancestor rollup at the origin.
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

impl CascadesEngine {
    // Compile-time specialization keeps the full rebuild free of incremental branches.
    fn run_tree<const INCREMENTAL: bool>(
        &mut self,
        tree: &Tree,
        layout: &LayerLayout,
        lc: &mut LayerCascades,
        entries: &mut Soa<EntryRow>,
        hits: &mut Soa<HitRow>,
        display_scale: f32,
    ) -> bool {
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let widget_ids = tree.records.widget_id();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        let root_prefix = build_cascade_prefix(TranslateScale::IDENTITY, None, false, false);

        let mut i: u32 = 0;
        while i < n {
            // Pop completed frames, rolling each up into its parent.
            while let Some(top) = self.stack.last() {
                if i < top.subtree_end {
                    break;
                }
                let popped = self.stack.pop().unwrap();
                finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, popped);
            }
            let (parent_transform, parent_clip, parent_dis, parent_inv, parent_prefix) =
                match self.stack.last() {
                    Some(p) => (
                        p.transform,
                        p.clip,
                        p.disabled,
                        p.invisible,
                        &p.cascade_prefix,
                    ),
                    None => (TranslateScale::IDENTITY, None, false, false, &root_prefix),
                };

            let iu = i as usize;
            let id = NodeId(i);
            let attrs = attrs_col[iu];
            let layout_core = layout_col[iu];

            let disabled = parent_dis || attrs.is_disabled();
            let owner_visible = layout_core.visibility().is_visible();
            let invisible = parent_inv || !owner_visible;

            let layout_rect = layout.rect[iu];
            // `.end()` strips the packed grid flag — downstream uses (walk
            // cursor, leaf compare) need the clean pre-order end.
            let subtree_end = ends[iu].end();
            let has_children = subtree_end != i + 1;
            if INCREMENTAL && lc.subtree_hashes[iu] == subtree_hashes[iu] {
                if let Some(parent) = self.stack.last_mut() {
                    parent.subtree_paint_rect =
                        parent.subtree_paint_rect.union(lc.subtree_paint_rects[iu]);
                }
                i = subtree_end;
                continue;
            }

            let screen_rect = parent_transform.apply_rect(layout_rect);
            let visible_rect = parent_clip.map_or(screen_rect, |c| screen_rect.intersect(c));
            // The transform descendants inherit *and* direct shapes paint
            // under (the `Panel::transform` contract): `parent ∘
            // self_anchored`. Computed once here — `transform_of` is a
            // sparse-column probe and `compose` is 3×mul+3×add, so the
            // `None` arm (most nodes have no transform) skips the compose
            // entirely, the steady-state path. `compute_paint_rect` reuses
            // this as its `shape_transform` rather than recomposing.
            //
            // Scale pivots about the node's own `layout_rect.min`, not the
            // cascade's (0, 0); `anchored_at` cancels the
            // `panel.min * (1 - scale)` drift a raw compose against
            // absolute-coord layout rects would introduce (identity-
            // preserving — no-op at `scale == 1`). See
            // `TranslateScale::anchored_at`.
            let node_transform = tree.transform_of(id);
            let desc_transform = match node_transform {
                Some(t) => parent_transform.compose(t.anchored_at(layout_rect.min)),
                None => parent_transform,
            };
            let clips = attrs.clip_mode().is_clip();
            // Encoder's clip mask is `rect.deflated_by(padding)`, pushed
            // **before** the body. Direct shapes and descendants both
            // paint inside it. Mirror that here so per-shape damage rects
            // and inherited child clips reflect what actually paints —
            // otherwise a TextEdit's tall text shape (extent = full
            // shaped buffer) reports damage well past the editor's rect
            // on every scroll tick.
            let shape_clip = if clips {
                let mask_local = layout_rect.deflated_by(layout_core.padding);
                let mask_screen = parent_transform.apply_rect(mask_local);
                Some(parent_clip.map_or(mask_screen, |c| mask_screen.intersect(c)))
            } else {
                parent_clip
            };
            let ctx = PaintRectCtx {
                tree,
                layout,
                node: id,
                layout_rect,
                visible_rect,
                padding: layout_core.padding,
                parent_transform,
                parent_clip,
                shape_clip,
                shape_transform: desc_transform,
                display_scale,
                clips,
                has_children,
            };
            let paint_rect = if INCREMENTAL {
                let old_span = lc.paint_arena.node_spans[iu];
                let paint_rect = compute_node_paint(ctx, owner_visible, &mut self.paint_scratch);
                let new_span = self.paint_scratch.node_spans[iu];
                if old_span.len != new_span.len {
                    return false;
                }
                lc.paint_arena.rows[old_span.range()]
                    .copy_from_slice(&self.paint_scratch.rows[new_span.range()]);
                paint_rect
            } else {
                compute_node_paint(ctx, owner_visible, &mut lc.paint_arena)
            };
            // Invisible nodes never paint, so seeding their subtree
            // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
            // from inflating the ancestor's `subtree_paint_rect` (and
            // killing the encoder's viewport / damage cull at that
            // ancestor). Visibility is in `cascade_input` regardless, so
            // damage tracking is unaffected.
            let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
            if INCREMENTAL {
                lc.subtree_hashes[iu] = subtree_hashes[iu];
            } else {
                lc.cascade_inputs[iu] = finish_cascade_input(parent_prefix, layout_rect, invisible);
                lc.subtree_ends[iu] = subtree_end;
            }
            lc.subtree_paint_rects[iu] = subtree_seed;

            // Descendants inherit the deflated-mask clip — same value the
            // direct shapes were clipped to above and the encoder pushes
            // before the body.
            let desc_clip = shape_clip;
            if !INCREMENTAL {
                let cascaded_off = disabled || invisible;
                let sense = if cascaded_off {
                    Sense::NONE
                } else {
                    attrs.sense()
                };
                let focusable = !cascaded_off && attrs.is_focusable();
                if sense != Sense::NONE || focusable {
                    hits.push(HitRow {
                        entry_idx: entries.len() as u32,
                        widget_id: widget_ids[iu],
                    });
                }
                entries.push(EntryRow {
                    rect: visible_rect,
                    sense,
                    focusable,
                    disabled,
                    layout_rect,
                    transform: parent_transform,
                });
            }

            if !has_children {
                // Leaf: no descendants, so no frame — its
                // `subtree_paint_rects` slot already holds the seed written
                // above; fold the seed straight into the parent accumulator
                // (a non-painting leaf's `Rect::ZERO` seed is `union`'s
                // identity). Skips a per-leaf Frame push/pop and the 32 B
                // prefix-hash work leaves could never hand to a child.
                if let Some(parent) = self.stack.last_mut() {
                    parent.subtree_paint_rect = parent.subtree_paint_rect.union(subtree_seed);
                }
            } else {
                self.stack.push(Frame {
                    transform: desc_transform,
                    clip: desc_clip,
                    disabled,
                    invisible,
                    subtree_end,
                    node_idx: iu,
                    subtree_paint_rect: subtree_seed,
                    cascade_prefix: build_cascade_prefix(
                        desc_transform,
                        shape_clip,
                        disabled,
                        invisible,
                    ),
                });
            }
            i += 1;
        }
        // Drain frames whose subtree extends to the end of the tree —
        // they never hit the `< top.subtree_end` exit at the loop head.
        while let Some(popped) = self.stack.pop() {
            finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, popped);
        }
        true
    }
}

#[inline]
fn compute_node_paint(ctx: PaintRectCtx<'_>, owner_visible: bool, arena: &mut PaintArena) -> Rect {
    if !owner_visible {
        arena.node_spans[ctx.node.idx()] = Span::new(arena.rows.len() as u32, 0);
        return Rect::ZERO;
    }
    compute_paint_rect(ctx, arena)
}

/// Ancestor-derived portion of the `cascade_input` hash — folded once
/// per stack frame at push time (32 B) and cloned per descendant. Split
/// out from the per-node suffix (`layout_rect`) so a tree-shaped UI
/// avoids re-hashing the parent context on every node.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::NoUninit)]
struct CascadePrefixBits {
    transform: [u32; 4],
    clip: [u32; 4],
}

#[inline]
fn build_cascade_prefix(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
) -> Hasher {
    let (clip, clip_present) = match parent_clip {
        Some(rect) => (
            [
                approx::canon_bits(rect.min.x),
                approx::canon_bits(rect.min.y),
                approx::canon_bits(rect.size.w),
                approx::canon_bits(rect.size.h),
            ],
            true,
        ),
        None => ([0; 4], false),
    };
    let flags = (clip_present as u32) | ((parent_dis as u32) << 1) | ((parent_inv as u32) << 2);
    let packed = CascadePrefixBits {
        transform: [
            approx::canon_bits(parent_transform.translation.x),
            approx::canon_bits(parent_transform.translation.y),
            approx::canon_bits(parent_transform.scale - 1.0),
            flags,
        ],
        clip,
    };
    let mut h = Hasher::new();
    h.pod(&packed);
    h
}

#[inline]
fn finish_cascade_input(prefix: &Hasher, layout_rect: Rect, invisible: bool) -> CascadeInputHash {
    let mut h = prefix.clone();
    approx::hash_visual_rect(layout_rect, &mut h);
    CascadeInputHash::pack(h.finish(), invisible)
}

/// Lift an owner-local rect into screen space: translate by the owner's
/// arranged origin, apply the relevant transform (`parent_transform`
/// for chrome / clip lift, `shape_transform` for shapes), then clip
/// to the ancestor clip. One source of truth for the three coord-
/// space hops the paint emit does.
#[inline]
fn lift_to_screen(local: Rect, origin: Vec2, t: TranslateScale, clip: Option<Rect>) -> Rect {
    let r = t.apply_rect(Rect {
        min: origin + local.min,
        size: local.size,
    });
    clip_screen(r, clip)
}

#[inline]
fn clip_screen(screen: Rect, clip: Option<Rect>) -> Rect {
    clip.map_or(screen, |c| screen.intersect(c))
}

/// Pad a text shape's screen rect by half a `TEXT_SCALE_STEP` of its
/// measured extent on each axis side, then re-clamp to `clip`.
///
/// The composer paints glyphs at the ladder-*snapped* scale
/// (`composer::snap_text_scale`), while the cascade lifts the rect at
/// the unsnapped scale. The painted block can be up to
/// `|snapped − cascade| ≤ STEP/2` longer per axis than the lifted
/// rect, which works out to `measured × STEP/2` of absolute screen
/// pixels per side — independent of cascade scale. A local-coord pad
/// would multiply by cascade and underflow at `cascade < 1`
/// (zoomed-out content), leaking glyph fringes past the damage rect.
/// Padding in screen space keeps damage covering the worst-case
/// painted extent at any zoom.
#[inline]
fn inflate_text_damage(screen: Rect, measured: Size, clip: Option<Rect>) -> Rect {
    // `screen` is already clipped, so a fully-off-clip run has collapsed
    // to zero on an axis (a zero-width box pinned at the clip edge). It
    // has no visible glyphs to pad; inflating it here would re-grow the
    // box *back across the clip edge*, fabricating a sub-pixel damage
    // sliver at the viewport edge for text that isn't on screen at all
    // (the "offscreen node casts a shadow at the window edge" bug). Leave
    // a non-paintable box empty — `is_paint_empty` also folds in the NaN
    // and float-boundary near-zero cases a bare `<= 0` compare would miss.
    if screen.is_paint_empty() {
        return screen;
    }
    let pad_w = measured.w * (TEXT_SCALE_STEP * 0.5);
    let pad_h = measured.h * (TEXT_SCALE_STEP * 0.5);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    match clip {
        Some(c) => inflated.intersect(c),
        None => inflated,
    }
}

/// Push one paint row and fold its screen rect into the running union
/// in a single step. [`compute_paint_rect`]'s invariant requires the
/// union to track exactly the set of pushed non-paint-empty rows;
/// doing both here makes the two legs impossible to desync at a call
/// site. A paint-empty screen (shape fully clipped away) still pushes
/// its row — damage matches rows by identity and needs the slot — but
/// stays out of the union, which would otherwise grow to include the
/// degenerate box pinned at the clip edge.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Option<Rect>, screen: Rect, hash: ContentHash) {
    if !screen.is_paint_empty() {
        *union = Some(union.map_or(screen, |a| a.union(screen)));
    }
    arena.rows.push(Paint { screen, hash });
}

/// Inputs to [`compute_paint_rect`], threaded from `run_tree`.
/// `shape_transform` (the `parent ∘ self_anchored` descendants also
/// inherit) and `clips` are computed once at the call site and passed
/// in so we don't re-probe the sparse `transform_of` column, recompose
/// the transform, or re-read the SoA `attrs` column — all showed up as
/// duplicate work in cascade profiling.
struct PaintRectCtx<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    node: NodeId,
    layout_rect: Rect,
    visible_rect: Rect,
    padding: Spacing,
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    shape_clip: Option<Rect>,
    shape_transform: TranslateScale,
    display_scale: f32,
    clips: bool,
    has_children: bool,
}

impl std::fmt::Debug for PaintRectCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintRectCtx")
            .field("node", &self.node)
            .field("layout_rect", &self.layout_rect)
            .field("visible_rect", &self.visible_rect)
            .field("padding", &self.padding)
            .field("parent_transform", &self.parent_transform)
            .field("parent_clip", &self.parent_clip)
            .field("shape_clip", &self.shape_clip)
            .field("shape_transform", &self.shape_transform)
            .field("display_scale", &self.display_scale)
            .field("clips", &self.clips)
            .field("has_children", &self.has_children)
            .finish_non_exhaustive()
    }
}

/// Emit every paint row for `node` — chrome at row 0 when present,
/// then direct shapes and child markers in record order — write the
/// covering [`Span`] into `node_spans[node]`, and return the
/// screen-space union of the pixel-producing rows — used locally as
/// the `subtree_paint_rects` seed for the encoder's cull. Damage
/// recomputes the same union from the `paint_arena` rows on demand
/// (its cold paths), so it isn't stored per node.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). Child markers are
/// pushed raw (zero screen, child `WidgetId` as hash) — they exist so
/// the damage diff sees the paint-order interleave; the child's pixels
/// are covered by its own rows.
///
/// # Invariant
///
/// The returned `Rect` is bit-identical to the screen-space union of
/// the non-paint-empty rows in
/// `arena.rows[paints_start..arena.rows.len()]` — the same union
/// `damage::union_screens` recomputes from the stored rows.
/// [`push_paint`] keeps the union and the pushed rows in lockstep;
/// child markers bypass it (zero rect, no pixels), and the chromeless
/// clip-only branch is the sole fold-without-push case (it contributes
/// a cull rect but emits no pixels).
fn compute_paint_rect(ctx: PaintRectCtx<'_>, arena: &mut PaintArena) -> Rect {
    let PaintRectCtx {
        tree,
        layout,
        node,
        layout_rect,
        visible_rect,
        padding,
        parent_transform,
        parent_clip,
        shape_clip,
        shape_transform,
        display_scale,
        clips,
        has_children,
    } = ctx;
    let paints_start = arena.rows.len() as u32;

    // `Option<Rect>` because zero-size sentinels bias `Rect::union`
    // toward the origin and an owner-rect seed would inflate damage
    // for chromeless shape hosts.
    let mut union: Option<Rect> = None;

    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };

    if let Some(bg) = tree.chrome(node) {
        let screen = if bg.shadow.is_noop() {
            visible_rect
        } else {
            let g = bg.shadow.geom();
            let chrome_local = owner_local.union(shadow_paint_rect_local(
                None,
                layout_rect.size,
                g.offset,
                g.blur,
                g.spread,
                bg.shadow.inset(),
            ));
            lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip)
        };
        push_paint(arena, &mut union, screen, bg.hash);
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        union = Some(visible_rect);
    }

    let has_shapes = tree.records.shape_span()[node.idx()].len > 0;
    if has_shapes || has_children {
        let text_span = layout.text_spans[node.idx()];
        let mut text_ord: u32 = 0;
        let shape_hashes = tree.shapes.hashes.as_slice();
        let widget_ids = tree.records.widget_id();
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            let (idx, s) = match item {
                TreeItem::ShapeRecord(idx, s) => (idx, s),
                TreeItem::Child(c) => {
                    arena.rows.push(Paint {
                        screen: Rect::ZERO,
                        hash: ContentHash(widget_ids[c.id.idx()].0),
                    });
                    continue;
                }
            };
            // Every direct text shape has one layout-derived entry, whether
            // measure produced it for a leaf or post-arrange shaping produced
            // it for a container.
            let screen = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    debug_assert!(
                        text_ord < text_span.len,
                        "cascade saw a text shape without a matching ShapedText entry",
                    );
                    let shaped = layout.text_shapes[(text_span.start + text_ord) as usize];
                    text_ord += 1;
                    let local = text_paint_bbox_local(
                        *local_origin,
                        *align,
                        padding,
                        layout_rect.size,
                        shaped.measured,
                    );
                    let screen = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    inflate_text_damage(screen, shaped.measured, shape_clip)
                }
                ShapeRecord::Polyline {
                    width,
                    cap,
                    join,
                    points,
                    bbox,
                    ..
                } => {
                    // The AA fringe is physical, so inflate only after the
                    // centerline and stroke width reach screen space.
                    let centerline = lift_to_screen(*bbox, layout_rect.min, shape_transform, None);
                    let screen = stroked_bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        (points.len > 2).then_some(*join),
                    );
                    clip_screen(screen, shape_clip)
                }
                ShapeRecord::Curve {
                    width, cap, bbox, ..
                }
                | ShapeRecord::Arc {
                    width, cap, bbox, ..
                } => {
                    let centerline = lift_to_screen(*bbox, layout_rect.min, shape_transform, None);
                    let screen = stroked_bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        None,
                    );
                    clip_screen(screen, shape_clip)
                }
                _ => lift_to_screen(
                    s.bbox_local(layout_rect.size),
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
            };
            push_paint(arena, &mut union, screen, shape_hashes[idx as usize]);
        }
        debug_assert_eq!(
            text_ord, text_span.len,
            "cascade text count differs from the node's shaped-text span",
        );
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union.unwrap_or(Rect::ZERO)
}

#[cfg(test)]
mod tests;
