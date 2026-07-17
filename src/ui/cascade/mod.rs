//! Per-frame post-arrange state.
//!
//! `CascadesEngine` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Forest, &Layout)` and produces
//! a fresh `Cascades` — per-tree per-node cascade rows plus a
//! global hit index, all populated in a single per-tree pre-order walk.
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
/// index into it. Written during [`compute_paint_rect`], reset together
/// each frame in [`Self::reset_for`].
#[derive(Debug, Default)]
pub(crate) struct PaintArena {
    /// One [`Paint`] row per chrome contribution (row 0 of a node's
    /// span when present), direct shape, or immediate-child marker,
    /// in record order per node. Pushed in pre-order paint order;
    /// cleared each frame.
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

/// One hit-test row. Stored as `Soa<EntryRow>` on
/// [`Cascades::entries`] so each field becomes its own contiguous
/// slice. Hit tests use [`Cascades::hit_entries`] to visit only rows
/// that can interact, reading `rect` and the relevant flags while
/// ignoring `widget_id` / `layout_rect` until a match surfaces. Same
/// cache argument as aperture's
/// `Tree.records: Soa<NodeRecord>`.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct EntryRow {
    /// Author-supplied id. Read once per hit-test match.
    pub widget_id: WidgetId,
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    /// Read for rows referenced by [`Cascades::hit_entries`].
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
    /// into `rect` (screen space): `rect == transform.apply_rect(layout_rect)`.
    /// Surfaced via `ResponseState::transform` so a widget can convert a
    /// surface-space pointer back into its own logical coordinates (e.g.
    /// caret hit-testing under a zoomed canvas) — `IDENTITY` when untransformed.
    pub transform: TranslateScale,
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
/// are produced together in a single [`run_tree`] pass, reset together
/// at frame start, and read together by the damage diff and encoder —
/// keeping them on one struct means there's exactly one indexing point
/// per layer and no chance of resetting one column but not another.
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
/// - [`Self::subtree_ends`] is read only by [`Cascades::is_within`]
///   ancestry lookups — sparse random access, never a walk, so it
///   must not fatten the walked columns.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`]s plus the `node_spans` index). Read only on damage's
///   per-shape legs (vacant insert, hash mismatch, paint-anim lookup),
///   so it sits behind a `node_spans[i]` indirection that damage's
///   subtree-skip fast path skips entirely. A node's **own** paint
///   extent (the former `Cascade.paint_rect`) is just the union of its
///   `paint_arena` rows — recomputed on demand on damage's cold paths,
///   not stored.
#[derive(Debug, Default)]
pub(crate) struct LayerCascades {
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
    /// `reset_for` time. Every node in `tree.records` pushes exactly
    /// one entry in [`run_tree`], so within the layer block the entry
    /// index is `entries_base + node.0`. Combined with the per-pass
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
    /// `entries.id`, …), keeping response lookups node-aligned while
    /// hit tests reach interactive rows through [`Self::hit_entries`].
    /// Layers append in paint order so reverse iteration yields
    /// topmost-first.
    pub(crate) entries: Soa<EntryRow>,
    /// Entry indices whose effective `sense` is nonempty or which are
    /// focusable, in the same paint order as [`Self::entries`]. Hit tests
    /// reverse-scan this compact list while response lookup retains the
    /// full node-aligned entry table.
    pub(crate) hit_entries: Vec<u32>,
    /// `WidgetId → Endpoint` lookup for hit-test consumers
    /// ([`crate::input::InputState::response_for`], capture / focus
    /// eviction). **Invariant: equals `SeenIds.curr` as observed at
    /// the end of the most recent `CascadesEngine::run`** — populated
    /// by `clone_from(&seen.curr)` in [`CascadesEngine::run`], no
    /// other writer. The snapshot is required (rather than reading
    /// `seen.curr` directly) because `response_for` is called during
    /// recording, and `SeenIds::pre_record` clears `curr` at the top
    /// of every record pass — `request_relayout`'s second pass needs
    /// to see pass A's entries while its own widgets are still being
    /// recorded into the freshly-cleared `curr`. `seen.prev` is the
    /// wrong fallback: it carries the previous *frame*'s data, not
    /// the previous *pass*'s. Pays one O(N) memcpy per cascade run
    /// in exchange for not paying an O(N) hashmap insert per widget.
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

    /// Reverse walk (topmost-first under the pre-order paint walk) returning
    /// the first entry whose rect contains `pos` and whose `gate(i)` passes.
    /// Shared by [`Self::hit_test`] and [`Self::hit_test_focusable`], which
    /// differ only in the per-entry gate column they consult.
    fn hit_first(&self, pos: Vec2, gate: impl Fn(usize) -> bool) -> Option<WidgetId> {
        let rects = self.entries.rect();
        let ids = self.entries.widget_id();
        for &entry_idx in self.hit_entries.iter().rev() {
            let i = entry_idx as usize;
            if gate(i) && rects[i].contains(pos) {
                return Some(ids[i]);
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
        let ids = self.entries.widget_id();
        let mut hover = None;
        let mut scroll = None;
        let mut pinch = None;
        for &entry_idx in self.hit_entries.iter().rev() {
            let i = entry_idx as usize;
            if !rects[i].contains(pos) {
                continue;
            }
            if hover.is_none() && hover_filter(senses[i]) {
                hover = Some(ids[i]);
            }
            if scroll.is_none() && scroll_filter(senses[i]) {
                scroll = Some(ids[i]);
            }
            if pinch.is_none() && pinch_filter(senses[i]) {
                pinch = Some(ids[i]);
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
}

impl CascadesEngine {
    /// Walk every tree in paint order; produce one `Cascade` row per
    /// node in each tree's slot, and index the rows that can receive
    /// pointer or focus input. Reads the layout pass's output, writes
    /// into `cascades`.
    /// Root placement (`RootSlot.placement`) is already baked into the
    /// arranged rects by the layout pass, so no parent-transform
    /// plumbing is needed here — trees never share NodeId space.
    #[profiling::function]
    pub(crate) fn run(&mut self, forest: &Forest, layout: &Layout, cascades: &mut Cascades) {
        let total: usize = forest.trees.0.iter().map(|t| t.records.len()).sum();
        cascades.entries.clear();
        cascades.entries.reserve(total);
        cascades.hit_entries.clear();

        for (layer, tree) in forest.trees.iter_paint_order() {
            let layer_layout = &layout.layers[layer];
            let n = tree.records.len();
            let entries_base = cascades.entries.len() as u32;
            cascades.layers[layer].reset_for(n, entries_base);
            self.stack.clear();
            run_tree(
                tree,
                layer_layout,
                &mut cascades.layers[layer],
                &mut cascades.entries,
                &mut cascades.hit_entries,
                &mut self.stack,
            );
            // Invariant guarding `Cascades::entry_idx_of`'s
            // `entries_base + node.0` arithmetic: every node in
            // `tree.records` must push exactly one `EntryRow`. An
            // early-continue / skip-invisible optimization inside
            // `run_tree` that doesn't push would silently shift every
            // later widget's entry by one.
            debug_assert_eq!(
                cascades.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree pushed {} entries for layer with {n} nodes — every record must push exactly one row to keep entries_base + node.0 valid",
                cascades.entries.len() as u32 - entries_base,
            );
        }

        // Snapshot `seen.curr` for inter-pass `response_for` lookups.
        // `request_relayout`'s second pass clears `curr` in
        // `pre_record` *before* the second pass's widgets call
        // `response_for(id)`, so the data has to live on `Cascades`
        // instead. `clone_from` reuses storage — one O(N) memcpy
        // replaces N per-widget hashmap inserts.
        cascades.by_id.clone_from(&forest.ids.curr);
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
            // The root's own id reaches no other hash —
            // `compute_hashes` folds only child ids into parents,
            // and a root has no parent — so a re-keyed root with
            // identical content would otherwise reuse cascades
            // whose `by_id` still maps the dead old id.
            h.write_u64(tree.records.widget_id()[slot.first_node.idx()].0);
            h.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            // Layer placement (anchor + measure cap) rides on
            // `RootSlot`, outside every node hash, yet it feeds
            // arrange. Fold it so a popup's pass-B flip/clamp —
            // which changes only the anchor while the body content
            // is identical — re-runs the cascade instead of reusing
            // pass A's pre-flip screen rects (else the popup paints
            // at the raw anchor until an unrelated content change
            // forces a recompute).
            approx::hash_visual_vec2(slot.placement.anchor, &mut h);
            match slot.placement.size {
                Some(size) => {
                    h.write_u8(1);
                    approx::hash_visual_size(size, &mut h);
                }
                None => h.write_u8(0),
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

fn run_tree(
    tree: &Tree,
    layout: &LayerLayout,
    lc: &mut LayerCascades,
    entries: &mut Soa<EntryRow>,
    hit_entries: &mut Vec<u32>,
    stack: &mut Vec<Frame>,
) {
    let n = tree.records.len() as u32;
    let layout_col = tree.records.layout();
    let attrs_col = tree.records.attrs();
    let widget_ids = tree.records.widget_id();
    let ends = tree.records.subtree_end();
    let root_prefix = build_cascade_prefix(TranslateScale::IDENTITY, None, false, false);

    let mut i: u32 = 0;
    while i < n {
        // Pop completed frames, rolling each up into its parent.
        while let Some(top) = stack.last() {
            if i < top.subtree_end {
                break;
            }
            let popped = stack.pop().unwrap();
            finalize_frame(stack, &mut lc.subtree_paint_rects, popped);
        }
        let (parent_transform, parent_clip, parent_dis, parent_inv, parent_prefix) =
            match stack.last() {
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
        let invisible = parent_inv || !layout_core.visibility().is_visible();

        let layout_rect = layout.rect[iu];
        // `.end()` strips the packed grid flag — downstream uses (walk
        // cursor, leaf compare) need the clean pre-order end.
        let subtree_end = ends[iu].end();
        let has_children = subtree_end != i + 1;
        let wid = widget_ids[iu];

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
        let paint_rect = compute_paint_rect(
            PaintRectCtx {
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
                clips,
                has_children,
            },
            &mut lc.paint_arena,
        );
        // Invisible nodes never paint, so seeding their subtree
        // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
        // from inflating the ancestor's `subtree_paint_rect` (and
        // killing the encoder's viewport / damage cull at that
        // ancestor). Visibility is in `cascade_input` regardless, so
        // damage tracking is unaffected.
        let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
        lc.cascade_inputs[iu] = finish_cascade_input(parent_prefix, layout_rect, invisible);
        lc.subtree_paint_rects[iu] = subtree_seed;
        lc.subtree_ends[iu] = subtree_end;

        // Descendants inherit the deflated-mask clip — same value the
        // direct shapes were clipped to above and the encoder pushes
        // before the body.
        let desc_clip = shape_clip;
        let cascaded_off = disabled || invisible;
        let sense = if cascaded_off {
            Sense::NONE
        } else {
            attrs.sense()
        };
        let focusable = !cascaded_off && attrs.is_focusable();
        if sense != Sense::NONE || focusable {
            hit_entries.push(entries.len() as u32);
        }
        entries.push(EntryRow {
            widget_id: wid,
            rect: visible_rect,
            sense,
            focusable,
            disabled,
            layout_rect,
            transform: parent_transform,
        });

        if !has_children {
            // Leaf: no descendants, so no frame — its
            // `subtree_paint_rects` slot already holds the seed written
            // above; fold the seed straight into the parent accumulator
            // (a non-painting leaf's `Rect::ZERO` seed is `union`'s
            // identity). Skips a per-leaf Frame push/pop and the 32 B
            // prefix-hash work leaves could never hand to a child.
            if let Some(parent) = stack.last_mut() {
                parent.subtree_paint_rect = parent.subtree_paint_rect.union(subtree_seed);
            }
        } else {
            stack.push(Frame {
                transform: desc_transform,
                clip: desc_clip,
                disabled,
                invisible,
                subtree_end,
                node_idx: iu,
                subtree_paint_rect: subtree_seed,
                cascade_prefix: build_cascade_prefix(
                    desc_transform,
                    desc_clip,
                    disabled,
                    invisible,
                ),
            });
        }
        i += 1;
    }
    // Drain frames whose subtree extends to the end of the tree —
    // they never hit the `< top.subtree_end` exit at the loop head.
    while let Some(popped) = stack.pop() {
        finalize_frame(stack, &mut lc.subtree_paint_rects, popped);
    }
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
    clip.map_or(r, |c| r.intersect(c))
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
            let (local, text_measured) = match s {
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
                    (local, Some(shaped.measured))
                }
                _ => (s.paint_bbox_local(layout_rect.size), None),
            };
            let mut screen = lift_to_screen(local, layout_rect.min, shape_transform, shape_clip);
            if let Some(measured) = text_measured {
                screen = inflate_text_damage(screen, measured, shape_clip);
            }
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
