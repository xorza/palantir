//! Per-frame post-arrange state.
//!
//! `CascadesEngine` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Forest, &Layout)` and produces
//! a fresh `Cascades` — per-tree per-node cascade rows plus a
//! global hit index, all populated in a single per-tree pre-order walk.
//! Downstream phases (damage diff, input hit-test, renderer encoder)
//! take `&Cascades` as their single frozen-state handle.

use crate::common::hash::Hasher;
use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::per_layer::PerLayer;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::seen_ids::{Endpoint, WidgetIdMap};
use crate::forest::shapes::record::{ShapeRecord, shadow_paint_rect_local, text_paint_bbox_local};
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::input::sense::Sense;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{rect::Rect, transform::TranslateScale};
use crate::text::TEXT_SCALE_STEP;
use glam::Vec2;
use soa_rs::{Soa, Soars};
use std::hash::Hasher as _;

/// One paintable contribution from a single node — either chrome (row 0
/// of the node's paint span when the node has chrome) or one direct
/// shape. Single source of truth for "did this pixel-producer change
/// since last frame?"
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    /// Screen-space rect after parent transform + clip.
    pub(crate) screen: Rect,
    /// Authoring hash. For chrome: `Tree.rollups.chrome[node]`.
    /// For shape: `Tree.shapes.hashes[shape_idx]`.
    pub(crate) hash: NodeHash,
}

/// Per-layer paint state: the unified [`Paint`] arena plus a per-node
/// index into it. Written during [`compute_paint_rect`], reset together
/// each frame in [`Self::reset_for`].
#[derive(Default)]
pub(crate) struct PaintArena {
    /// One [`Paint`] row per chrome contribution (row 0 of a node's
    /// span when present) or shape contribution. Pushed in pre-order
    /// paint order; cleared each frame.
    pub(crate) rows: Vec<Paint>,
    /// Per-node [`Span`] into [`Self::rows`]. Empty span
    /// (`Span::default()`) means the node paints nothing — replaces
    /// the old `rollups.paints` bitset.
    pub(crate) node_spans: Vec<Span>,
}

impl PaintArena {
    /// Reset both columns for a new frame. `n_nodes` sizes
    /// `node_spans` (zero-init to `Span::default()`); `rows` is cleared
    /// and reserved for the expected upper bound.
    pub(crate) fn reset_for(&mut self, n_nodes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.clear();
        self.node_spans.resize(n_nodes, Span::default());
    }
}

/// One hit-test row. Stored as `Soa<EntryRow>` on
/// [`Cascades::entries`] so each field becomes its own contiguous
/// slice — the hot reverse-scan in `hit_test*` reads `rect` and the
/// flags but ignores `widget_id` / `layout_rect` until a match
/// surfaces. Same cache argument as palantir's
/// `Tree.records: Soa<NodeRecord>`.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct EntryRow {
    /// Author-supplied id. Read once per hit-test match.
    pub widget_id: WidgetId,
    /// Visible screen rect (post-transform, clipped by ancestor clip).
    /// Hit-test reads every row.
    pub rect: Rect,
    /// Pointer interactions this row participates in (`HOVER` / `CLICK`
    /// / `DRAG` / `SCROLL`). Hit-test reads every row.
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
}

struct Frame {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    /// True when this node's inherited context (parent transform / clip
    /// / disabled / invisible) and its own arranged origin + authoring
    /// are unchanged from last frame — so a descendant with an unchanged
    /// `subtree_hash` and origin can be skipped (its cascade output is
    /// provably identical). Always false on the full-recompute path.
    ctx_unchanged: bool,
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

/// All per-layer cascade state grouped on one struct. The
/// `cascade_inputs`, `subtree_paint_rects`, and `paint_arena` columns
/// are produced together in a single [`run_tree`] pass, reset together
/// at frame start, and read together by the damage diff and encoder —
/// keeping them on one struct means there's exactly one indexing point
/// per layer and no chance of resetting one column but not another.
///
/// ## Columnar split
///
/// The per-node data is deliberately divided three ways, driven by
/// who reads what together:
///
/// - [`Self::cascade_inputs`] is the only datum on the per-node hot
///   path: the encoder reads `cascade_input.invisible()` for every
///   node it walks, and damage compares the full u64 on its skip /
///   descend arms. At 8 B/node the encoder's per-frame walk and
///   damage's scan stay cache-dense.
/// - [`Self::subtree_paint_rects`] is read only by the encoder cull.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`]s plus the `node_spans` index). Read only on damage's
///   per-shape legs (vacant insert, hash mismatch, paint-anim lookup),
///   so it sits behind a `node_spans[i]` indirection that damage's
///   subtree-skip fast path skips entirely. A node's **own** paint
///   extent (the former `Cascade.paint_rect`) is just the union of its
///   `paint_arena` rows — recomputed on demand on damage's cold paths,
///   not stored.
#[derive(Default)]
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
    /// `cascade_inputs` and `subtree_paint_rects` are cleared and
    /// reserved (filled by per-node pushes during the walk);
    /// `paint_arena` columns reset according to their own sizing rules.
    pub(crate) fn reset_for(&mut self, n_nodes: usize, entries_base: u32) {
        self.cascade_inputs.clear();
        self.cascade_inputs.reserve(n_nodes);
        self.subtree_paint_rects.clear();
        self.subtree_paint_rects.reserve(n_nodes);
        self.paint_arena.reset_for(n_nodes);
        self.entries_base = entries_base;
    }
}

/// Read-only artifact of `CascadesEngine::run`. Holds per-layer
/// cascade state (per-node rows, subtree rollups, paint arena — see
/// [`LayerCascades`]) plus the [`Self::by_id`] hit-lookup snapshot.
#[derive(Default)]
pub(crate) struct Cascades {
    pub(crate) layers: PerLayer<LayerCascades>,
    /// Pre-order hit-test rows in SoA form — each field is its own
    /// contiguous slice (`entries.rect()`, `entries.sense()`,
    /// `entries.widget_id()`, …) so the hot reverse-scan in
    /// `hit_test*` only pulls rect + flags into cache and pays the
    /// `WidgetId` / `layout_rect` load only on a match. Layers
    /// append in paint order so reverse iteration yields topmost-
    /// first.
    pub(crate) entries: Soa<EntryRow>,
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
    /// Push a hit-test row to the global SoA. Within a layer the
    /// pushes happen in `NodeId` order (one per [`run_tree`]
    /// iteration), so `LayerCascades::entries_base + node.0` is
    /// always the global entry index of the row — no parallel
    /// `WidgetId → u32` map needed.
    #[inline]
    fn push_entry(&mut self, row: EntryRow) {
        self.entries.push(row);
    }

    /// Global entry index of the widget last recorded under `id`,
    /// or `None` if `id` isn't in the most recent cascade run.
    #[inline]
    pub(crate) fn entry_idx_of(&self, id: WidgetId) -> Option<u32> {
        let ep = self.by_id.get(&id)?;
        Some(self.layers[ep.layer].entries_base + ep.node.0)
    }

    /// True when `id` appears in the most recent cascade run.
    #[inline]
    pub(crate) fn contains_widget(&self, id: WidgetId) -> bool {
        self.by_id.contains_key(&id)
    }

    /// Reverse-iter entries → topmost-first under pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for
    /// hover, clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        let rects = self.entries.rect();
        let senses = self.entries.sense();
        let ids = self.entries.widget_id();
        for i in (0..rects.len()).rev() {
            if filter(senses[i]) && rects[i].contains(pos) {
                return Some(ids[i]);
            }
        }
        None
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
        for i in (0..rects.len()).rev() {
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
        let rects = self.entries.rect();
        let focusables = self.entries.focusable();
        let ids = self.entries.widget_id();
        for i in (0..rects.len()).rev() {
            if focusables[i] && rects[i].contains(pos) {
                return Some(ids[i]);
            }
        }
        None
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitTargets {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: Option<WidgetId>,
    pub(crate) pinch: Option<WidgetId>,
}

/// Per-node reuse-gate inputs, snapshotted each frame so the next frame
/// can decide — per subtree — whether the cascade output is unchanged.
/// NodeId-indexed (parallel to `Tree::records`), one `Vec` per layer on
/// [`CascadesEngine::prev_snap`]. Self-contained: holds every datum the
/// gate and the structure check read, so they touch one array.
#[derive(Clone, Copy, Debug, Default)]
struct CascadeSnapshot {
    /// `tree.rollups.node[i]` — own authoring. A match means this node's
    /// own transform / clip / disabled / visibility / shapes / chrome
    /// are unchanged, so it hands identical inherited context to its
    /// children even when a deeper descendant changed.
    node_hash: NodeHash,
    /// `tree.rollups.subtree[i]` — authoring of the whole subtree. A
    /// match, *with* unchanged inherited context and origin, means the
    /// entire subtree's cascade output is identical and can be copied.
    subtree_hash: NodeHash,
    /// `layout.rect[i]` — arranged rect. Origin is an arrange *output*,
    /// not folded into `subtree_hash`, so a Fill-sibling reflow can move
    /// a node whose authoring is unchanged; the gate must compare it.
    rect: Rect,
    /// `tree.records.widget_id()[i]` — identity at this NodeId. Compared
    /// across frames to confirm the NodeId → widget mapping is stable;
    /// if not, NodeId-indexed reuse is invalid and the frame falls back
    /// to a full recompute.
    widget_id: WidgetId,
}

/// Walk telemetry: how many subtrees were skipped (bulk-copied) vs nodes
/// recomputed. Read by tests to assert the skip gate actually fires;
/// the gated [`CascadesEngine::dbg`] field is the only reader.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WalkStats {
    /// Whether `run` took the incremental path (vs full recompute).
    /// Test-only: set in `run`'s `cfg(test)` block and read by tests via
    /// [`CascadesEngine::dbg`]; the walk itself never touches it.
    #[cfg(test)]
    pub(crate) incremental: bool,
    pub(crate) skipped: u32,
    pub(crate) recascaded: u32,
}

/// Previous frame's reuse data for one layer, handed to [`run_tree`].
#[derive(Clone, Copy)]
struct PrevTree<'a> {
    cascades: &'a Cascades,
    snap: &'a [CascadeSnapshot],
}

#[derive(Default)]
pub(crate) struct CascadesEngine {
    stack: Vec<Frame>,
    /// Previous frame's `Cascades`, swapped out of `layout.cascades` at
    /// the top of each `run` so the walk can read it while rebuilding
    /// into the freed buffer. The reuse source for skipped subtrees.
    /// Boxed so it doesn't enlarge `Ui` inline — it's only touched when
    /// the cascade actually runs, never on a Stage-0-skipped idle frame,
    /// so the indirection is off the hot path and the smaller `Ui`
    /// keeps the other passes' fields cache-resident.
    prev: Box<Cascades>,
    /// Previous frame's per-node gate snapshots (read by the skip gate),
    /// one `Vec` per layer. [`Self::snap`] is this frame's write target;
    /// the two are swapped at the end of each `run` so `prev_snap`
    /// always describes the last frame. Boxed like [`Self::prev`].
    prev_snap: Box<PerLayer<Vec<CascadeSnapshot>>>,
    /// This frame's gate snapshots, written inline during the walk
    /// (folded in rather than a separate post-pass over the rollups).
    /// Swapped into `prev_snap` at the end of `run`.
    snap: Box<PerLayer<Vec<CascadeSnapshot>>>,
    /// False until the first `run` populates `prev` / `prev_snap`; gates
    /// the incremental path so the first frame recomputes fully.
    valid: bool,
    #[cfg(test)]
    pub(crate) dbg: WalkStats,
}

impl CascadesEngine {
    /// Walk every tree in paint order, producing one cascade row per
    /// node plus a global hit entry per node, into `layout.cascades`.
    ///
    /// Cross-frame reuse (O5 stage 1): when the tree structure is stable
    /// (same NodeId → `WidgetId` mapping as last frame), the walk skips
    /// any subtree whose authoring (`subtree_hash`), inherited context,
    /// and arranged origin are all unchanged — bulk-copying last frame's
    /// rows instead of recomputing them — and recomputes the rest.
    /// Otherwise it recomputes every node from scratch. Both paths
    /// produce byte-identical output; a `#[cfg(test)]` cross-check
    /// asserts that on every incremental frame.
    #[profiling::function]
    pub(crate) fn run(&mut self, forest: &Forest, layout: &mut Layout) {
        let Layout { layers, cascades } = layout;
        // Swap last frame's cascades out of `layout.cascades` so the
        // walk reads it (`self.prev`) while rebuilding into the freed
        // buffer (`cascades` now holds the frame-before-last's stale
        // rows, fully overwritten below).
        std::mem::swap(&mut *self.prev, cascades);

        let incremental = self.valid && structure_matches(forest, &self.prev_snap);
        let prev = incremental.then_some((&*self.prev, &*self.prev_snap));
        // The walk writes this frame's gate snapshots into `self.snap`
        // inline (no separate pass over the rollups).
        let stats = run_pass(
            forest,
            layers,
            cascades,
            prev,
            &mut self.stack,
            &mut self.snap,
        );

        // Snapshot `seen.curr` for inter-pass `response_for` lookups.
        // `request_relayout`'s second pass clears `curr` in `pre_record`
        // *before* its widgets call `response_for(id)`, so the data has
        // to live on `Cascades` instead. `clone_from` reuses storage —
        // one O(N) memcpy replaces N per-widget hashmap inserts.
        cascades.by_id.clone_from(&forest.ids.curr);

        // `snap` now holds this frame's gate inputs; promote it to
        // `prev_snap` for next frame. Combined with the `prev`/`cascades`
        // swap above (which next frame moves `cascades` into `prev`),
        // both reuse sources describe this frame by next frame's walk.
        std::mem::swap(&mut self.prev_snap, &mut self.snap);
        self.valid = true;

        #[cfg(test)]
        {
            self.dbg = WalkStats {
                incremental,
                ..stats
            };
            if incremental {
                // Every reuse frame is verified against a from-scratch
                // recompute: the incremental walk must be byte-identical.
                cross_check(forest, layers, cascades, &mut self.stack);
            }
        }
        #[cfg(not(test))]
        let _ = stats;
    }
}

/// True when every layer's NodeId → `WidgetId` mapping is identical to
/// the snapshot — the precondition for NodeId-indexed cross-frame reuse.
/// A changed node count or any shifted id means the prev arrays no
/// longer line up, so the caller must recompute fully.
fn structure_matches(forest: &Forest, prev_snap: &PerLayer<Vec<CascadeSnapshot>>) -> bool {
    for (layer, tree) in forest.iter_paint_order() {
        let snap = &prev_snap[layer];
        let wids = tree.records.widget_id();
        if snap.len() != wids.len() {
            return false;
        }
        if snap.iter().zip(wids).any(|(s, &w)| s.widget_id != w) {
            return false;
        }
    }
    true
}

/// Per-layer walk driver shared by the live run and the test
/// cross-check. `prev: Some` enables the incremental skip; `None`
/// recomputes every node. Returns aggregate [`WalkStats`].
fn run_pass(
    forest: &Forest,
    layers: &PerLayer<LayerLayout>,
    out: &mut Cascades,
    prev: Option<(&Cascades, &PerLayer<Vec<CascadeSnapshot>>)>,
    stack: &mut Vec<Frame>,
    snap_out: &mut PerLayer<Vec<CascadeSnapshot>>,
) -> WalkStats {
    let total: usize = forest.trees.iter().map(|t| t.records.len()).sum();
    out.entries.clear();
    out.entries.reserve(total);

    let mut stats = WalkStats::default();
    for (layer, tree) in forest.iter_paint_order() {
        let layer_layout = &layers[layer];
        let n = tree.records.len();
        let entries_base = out.entries.len() as u32;
        out.layers[layer].reset_for(n, entries_base);
        let snap_layer = &mut snap_out[layer];
        snap_layer.clear();
        snap_layer.reserve(n);
        stack.clear();
        let prev_tree = prev.map(|(pc, ps)| PrevTree {
            cascades: pc,
            snap: ps[layer].as_slice(),
        });
        run_tree(
            tree,
            layer_layout,
            out,
            layer,
            stack,
            prev_tree,
            snap_layer,
            &mut stats,
        );
        // Invariant guarding `Cascades::entry_idx_of`'s
        // `entries_base + node.0` arithmetic: every node in
        // `tree.records` must push exactly one `EntryRow` (whether
        // recomputed or copied). A skip that miscounts, or an
        // early-continue that doesn't push, would silently shift every
        // later widget's entry by one. Release `assert!` — the operands
        // are already loaded, the equality is a single compare.
        assert_eq!(
            out.entries.len() as u32 - entries_base,
            n as u32,
            "run_tree produced {} entries for layer with {n} nodes — every record must yield exactly one row to keep entries_base + node.0 valid",
            out.entries.len() as u32 - entries_base,
        );
        // The folded snapshot must likewise cover every node exactly
        // once, so next frame's NodeId-indexed gate lines up.
        debug_assert_eq!(
            snap_layer.len(),
            n,
            "run_tree wrote {} snapshots for {n} nodes",
            snap_layer.len(),
        );
    }
    stats
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
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

/// The per-layer handles the walk threads through every node: the source
/// `tree` + arranged `layout`, and which `layer`'s `Cascades` columns to
/// write. `Copy` (all refs + a `Layer`), so `recascade_node` takes it by
/// value.
#[derive(Clone, Copy)]
struct LayerView<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    layer: Layer,
}

/// What a node inherits from its parent frame (or the root defaults):
/// the composed `transform` / `clip` it extends, the `disabled` /
/// `invisible` flags it ORs into, the hash `prefix` it seeds its
/// `cascade_input` from, and whether all of that is unchanged since last
/// frame (`ctx_ok`) — which, paired with an unchanged subtree, is what
/// lets a child be skipped.
#[derive(Clone, Copy)]
struct Inherited<'a> {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    prefix: &'a Hasher,
    ctx_ok: bool,
}

// `run_tree` drives the walk skeleton (pop → skip-gate → recompute);
// `recascade_node` does the per-node computation. The skeleton still
// takes 8 args — read inputs, the `stack` scratch, the `prev` reuse
// source, and three output sinks (cascades, folded snapshot, stats),
// which alias disjoint storage and would only gain reborrow ceremony if
// bundled.
#[allow(clippy::too_many_arguments)]
fn run_tree(
    tree: &Tree,
    layout: &LayerLayout,
    cascades: &mut Cascades,
    layer: Layer,
    stack: &mut Vec<Frame>,
    prev: Option<PrevTree<'_>>,
    snap_out: &mut Vec<CascadeSnapshot>,
    stats: &mut WalkStats,
) {
    let n = tree.records.len() as u32;
    let ends = tree.records.subtree_end();
    let root_prefix = build_cascade_prefix(TranslateScale::IDENTITY, None, false, false);
    let view = LayerView {
        tree,
        layout,
        layer,
    };

    let mut i: u32 = 0;
    while i < n {
        // Pop completed frames, rolling each up into its parent.
        while let Some(top) = stack.last() {
            if i < top.subtree_end {
                break;
            }
            let popped = stack.pop().unwrap();
            finalize_frame(
                stack,
                &mut cascades.layers[layer].subtree_paint_rects,
                popped,
            );
        }
        let iu = i as usize;
        let layout_rect = layout.rect[iu];
        // `.end()` strips the packed grid flag — downstream uses (walk
        // cursor, leaf compare) need the clean pre-order end.
        let subtree_end = ends[iu].end();
        // Root inherits a constant identity context, so it's always
        // "unchanged" — gated on `prev` so the very first frame (no prev
        // to copy) still recomputes.
        let parent_ctx_ok = stack.last().map_or(prev.is_some(), |p| p.ctx_unchanged);

        // Incremental skip: when the inherited context, this subtree's
        // authoring (`subtree_hash`), and its arranged rect all match
        // last frame, the whole subtree's cascade output is identical —
        // bulk-copy it and jump past it. `subtree_hash` folds every
        // descendant's authoring (incl. transforms, so a scroll shift
        // dirties it); the rect compare catches a Fill-sibling reflow it
        // can't see. Gated on `parent_ctx_ok` first so a node under a
        // changed ancestor never even loads its snapshot.
        if let Some(prev) = prev
            && parent_ctx_ok
        {
            let snap = prev.snap[iu];
            if tree.rollups.subtree[iu] == snap.subtree_hash && layout_rect == snap.rect {
                copy_subtree(prev, cascades, snap_out, layer, iu, subtree_end as usize);
                if let Some(top) = stack.last_mut() {
                    top.subtree_paint_rect = top
                        .subtree_paint_rect
                        .union(prev.cascades.layers[layer].subtree_paint_rects[iu]);
                }
                stats.skipped += 1;
                i = subtree_end;
                continue;
            }
        }
        stats.recascaded += 1;
        // Built here (after the gate) rather than up top so the skip
        // path's `stack.last_mut()` doesn't collide with the
        // `&p.cascade_prefix` borrow this holds.
        let inherited = match stack.last() {
            Some(p) => Inherited {
                transform: p.transform,
                clip: p.clip,
                disabled: p.disabled,
                invisible: p.invisible,
                prefix: &p.cascade_prefix,
                ctx_ok: parent_ctx_ok,
            },
            None => Inherited {
                transform: TranslateScale::IDENTITY,
                clip: None,
                disabled: false,
                invisible: false,
                prefix: &root_prefix,
                ctx_ok: parent_ctx_ok,
            },
        };
        let frame = recascade_node(view, NodeId(i), inherited, prev, cascades, snap_out);
        stack.push(frame);
        i += 1;
    }
    // Drain frames whose subtree extends to the end of the tree —
    // they never hit the `< top.subtree_end` exit at the loop head.
    while let Some(popped) = stack.pop() {
        finalize_frame(
            stack,
            &mut cascades.layers[layer].subtree_paint_rects,
            popped,
        );
    }
}

/// Recompute one node's cascade output from scratch — its `cascade_input`
/// hash, hit entry, paint rows + subtree-paint seed, and the gate
/// snapshot for next frame — and return the [`Frame`] the caller pushes
/// (carrying this node's composed transform / clip / flags and its
/// context-unchanged bit down to its children). The recompute arm of the
/// walk; a skipped subtree bypasses all of this via [`copy_subtree`].
fn recascade_node(
    view: LayerView<'_>,
    node: NodeId,
    inherited: Inherited<'_>,
    prev: Option<PrevTree<'_>>,
    cascades: &mut Cascades,
    snap_out: &mut Vec<CascadeSnapshot>,
) -> Frame {
    let LayerView {
        tree,
        layout,
        layer,
    } = view;
    let iu = node.idx();
    let layout_rect = layout.rect[iu];
    let subtree_end = tree.subtree_end_of(iu);
    let subtree_hash = tree.rollups.subtree[iu];
    let node_hash = tree.rollups.node[iu];
    let attrs = tree.records.attrs()[iu];
    let layout_core = &tree.records.layout()[iu];

    let disabled = inherited.disabled || attrs.is_disabled();
    let invisible = inherited.invisible || !layout_core.visibility().is_visible();
    let wid = tree.records.widget_id()[iu];

    let screen_rect = inherited.transform.apply_rect(layout_rect);
    let visible_rect = inherited
        .clip
        .map_or(screen_rect, |c| screen_rect.intersect(c));
    // The transform descendants inherit *and* direct shapes paint under
    // (the `Panel::transform` contract): `parent ∘ self_anchored`.
    // `transform_of` is a sparse-column probe and `compose` is 3×mul+3×add,
    // so the `None` arm (most nodes have no transform) skips the compose
    // entirely — the steady-state path. `compute_paint_rect` reuses this
    // as its `shape_transform` rather than recomposing.
    //
    // Scale pivots about the node's own `layout_rect.min`, not (0, 0);
    // `anchored_at` cancels the `panel.min * (1 - scale)` drift a raw
    // compose against absolute-coord layout rects would introduce
    // (identity-preserving — no-op at `scale == 1`).
    let node_transform = tree.transform_of(node);
    let desc_transform = match node_transform {
        Some(t) => inherited.transform.compose(t.anchored_at(layout_rect.min)),
        None => inherited.transform,
    };
    let clips = attrs.clip_mode().is_clip();
    // Encoder's clip mask is `rect.deflated_by(padding)`, pushed
    // **before** the body; direct shapes and descendants both paint
    // inside it. Mirror that here so per-shape damage rects and inherited
    // child clips reflect what actually paints — otherwise a TextEdit's
    // tall text shape reports damage well past the editor's rect on every
    // scroll tick.
    let shape_clip = if clips {
        let padding = layout_core.padding;
        let mask_local = layout_rect.deflated_by(padding);
        let mask_screen = inherited.transform.apply_rect(mask_local);
        Some(
            inherited
                .clip
                .map_or(mask_screen, |c| mask_screen.intersect(c)),
        )
    } else {
        inherited.clip
    };
    let paint_rect = compute_paint_rect(
        PaintRectCtx {
            tree,
            layout,
            node,
            layout_rect,
            parent_transform: inherited.transform,
            parent_clip: inherited.clip,
            shape_clip,
            shape_transform: desc_transform,
            clips,
        },
        &mut cascades.layers[layer].paint_arena,
    );
    // Invisible nodes never paint, so seeding their subtree rollup with
    // `Rect::ZERO` keeps a long-lived hidden subtree from inflating the
    // ancestor's `subtree_paint_rect` (and killing the encoder's viewport
    // / damage cull there). Visibility is in `cascade_input` regardless.
    let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
    cascades.layers[layer]
        .cascade_inputs
        .push(finish_cascade_input(
            inherited.prefix,
            layout_rect,
            invisible,
        ));
    cascades.layers[layer]
        .subtree_paint_rects
        .push(subtree_seed);

    // Descendants inherit the deflated-mask clip — the same value the
    // direct shapes were clipped to and the encoder pushes before the
    // body.
    let desc_clip = shape_clip;
    let cascaded_off = disabled || invisible;
    let sense = if cascaded_off {
        Sense::NONE
    } else {
        attrs.sense()
    };
    let focusable = !cascaded_off && attrs.is_focusable();
    cascades.push_entry(EntryRow {
        widget_id: wid,
        rect: visible_rect,
        sense,
        focusable,
        disabled,
        layout_rect,
    });

    // Stamp this node's gate inputs for next frame (rollups + rect are
    // already in cache from the work above). A skip copies the whole
    // subtree's snapshots in `copy_subtree`, so every node is written
    // exactly once, in NodeId order.
    snap_out.push(CascadeSnapshot {
        node_hash,
        subtree_hash,
        rect: layout_rect,
        widget_id: wid,
    });

    // Leaves can't be a parent prefix for anyone — skip the 32 B
    // prefix-hash work, push a fresh-state `Hasher` placeholder
    // (`Hasher::new()` is just `FxHasher { hash: 0 }`, ~free).
    let is_leaf = subtree_end == node.0 + 1;
    let cascade_prefix = if is_leaf {
        Hasher::new()
    } else {
        build_cascade_prefix(desc_transform, desc_clip, disabled, invisible)
    };
    // Children inherit an unchanged context iff the inherited context
    // already was, this node's own authoring (`node_hash` — its transform
    // / clip / disabled / visibility) is unchanged, and — *only* when the
    // node passes a rect-derived value down — its arranged rect is
    // unchanged. A node feeds its rect into its children's context only
    // via a transform (anchored at its origin) or a clip (screen clip =
    // parent·rect); a plain container that merely resized hands children
    // an unchanged transform/clip, so its rect is irrelevant to them. A
    // child that itself *moved* is still caught by the skip gate's own
    // rect compare. A deeper subtree change leaves this true (so the
    // changed node's siblings stay skippable). Short-circuit on
    // `inherited.ctx_ok` so a node under a changed ancestor doesn't load
    // its snapshot just to discard the result.
    let ctx_depends_on_rect = node_transform.is_some() || clips;
    let child_ctx_ok = match prev {
        Some(prev) if inherited.ctx_ok => {
            let snap = prev.snap[iu];
            node_hash == snap.node_hash && (!ctx_depends_on_rect || layout_rect == snap.rect)
        }
        _ => false,
    };
    Frame {
        transform: desc_transform,
        clip: desc_clip,
        disabled,
        invisible,
        ctx_unchanged: child_ctx_ok,
        subtree_end,
        node_idx: iu,
        subtree_paint_rect: subtree_seed,
        cascade_prefix,
    }
}

/// Bulk-copy the cascade output for the subtree `[start, end)` from the
/// previous frame into `out`. Every column the recompute path would
/// produce for these nodes is byte-identical to last frame (the skip
/// gate guarantees it), so it's memcpy'd rather than recomputed.
fn copy_subtree(
    prev: PrevTree<'_>,
    out: &mut Cascades,
    snap_out: &mut Vec<CascadeSnapshot>,
    layer: Layer,
    start: usize,
    end: usize,
) {
    // The subtree's gate snapshot is unchanged too (same authoring +
    // ctx + rect ⇒ same `node_hash`/`subtree_hash`/`rect`/`widget_id`),
    // so it carries over verbatim from last frame.
    snap_out.extend_from_slice(&prev.snap[start..end]);
    let pl = &prev.cascades.layers[layer];
    {
        let cl = &mut out.layers[layer];
        cl.cascade_inputs
            .extend_from_slice(&pl.cascade_inputs[start..end]);
        cl.subtree_paint_rects
            .extend_from_slice(&pl.subtree_paint_rects[start..end]);
        // Paint rows are packed in pre-order, so an earlier changed
        // sibling can shift this subtree's base offset. Copy the rows,
        // then rebase each node's span by the prev→new offset delta. The
        // subtree's rows are contiguous in `[start, end)` pre-order:
        // from node `start`'s span start to node `end`'s (the first node
        // past the subtree), or the row tail when the subtree ends the
        // tree.
        let node_count = pl.paint_arena.node_spans.len();
        let src_start = pl.paint_arena.node_spans[start].start as usize;
        let src_end = if end < node_count {
            pl.paint_arena.node_spans[end].start as usize
        } else {
            pl.paint_arena.rows.len()
        };
        let delta = cl.paint_arena.rows.len() as i64 - src_start as i64;
        cl.paint_arena
            .rows
            .extend_from_slice(&pl.paint_arena.rows[src_start..src_end]);
        for j in start..end {
            let s = pl.paint_arena.node_spans[j];
            cl.paint_arena.node_spans[j] = Span::new((s.start as i64 + delta) as u32, s.len);
        }
    }
    // Hit entries are one global Soa across layers; copy this subtree's
    // rows from the prev frame at the same per-layer base (NodeId stable
    // ⇒ same `entries_base` ⇒ same index).
    let base = pl.entries_base as usize;
    let pe = &prev.cascades.entries;
    for j in start..end {
        let k = base + j;
        out.push_entry(EntryRow {
            widget_id: pe.widget_id()[k],
            rect: pe.rect()[k],
            sense: pe.sense()[k],
            focusable: pe.focusable()[k],
            disabled: pe.disabled()[k],
            layout_rect: pe.layout_rect()[k],
        });
    }
}

/// Oracle for the incremental path: recompute the whole cascade from
/// scratch into a throwaway buffer and assert it's byte-identical to
/// what the reuse walk produced. Runs on every incremental frame under
/// test, so the entire frame-driving test suite verifies reuse
/// correctness across whatever topologies it exercises.
#[cfg(test)]
fn cross_check(
    forest: &Forest,
    layers: &PerLayer<LayerLayout>,
    built: &Cascades,
    stack: &mut Vec<Frame>,
) {
    let mut scratch = Cascades::default();
    let mut scratch_snap = PerLayer::<Vec<CascadeSnapshot>>::default();
    run_pass(forest, layers, &mut scratch, None, stack, &mut scratch_snap);
    scratch.by_id.clone_from(&forest.ids.curr);
    assert_cascades_eq(built, &scratch);
}

/// Field-by-field equality of two `Cascades` (neither it nor its
/// columns derive `PartialEq` — `entries` is a `Soa`). Used only by
/// [`cross_check`].
#[cfg(test)]
fn assert_cascades_eq(got: &Cascades, want: &Cascades) {
    for (layer, gl) in got.layers.iter_paint_order() {
        let wl = &want.layers[layer];
        assert_eq!(
            gl.cascade_inputs, wl.cascade_inputs,
            "cascade_inputs mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.subtree_paint_rects, wl.subtree_paint_rects,
            "subtree_paint_rects mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.paint_arena.rows, wl.paint_arena.rows,
            "paint rows mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.paint_arena.node_spans, wl.paint_arena.node_spans,
            "node_spans mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.entries_base, wl.entries_base,
            "entries_base mismatch @ {layer:?}"
        );
    }
    let (ge, we) = (&got.entries, &want.entries);
    assert_eq!(ge.len(), we.len(), "entries len mismatch");
    assert_eq!(ge.widget_id(), we.widget_id(), "entries.widget_id mismatch");
    assert_eq!(ge.rect(), we.rect(), "entries.rect mismatch");
    assert_eq!(ge.sense(), we.sense(), "entries.sense mismatch");
    assert_eq!(ge.focusable(), we.focusable(), "entries.focusable mismatch");
    assert_eq!(ge.disabled(), we.disabled(), "entries.disabled mismatch");
    assert_eq!(
        ge.layout_rect(),
        we.layout_rect(),
        "entries.layout_rect mismatch"
    );
}

/// Ancestor-derived portion of the `cascade_input` hash — folded once
/// per stack frame at push time (32 B) and cloned per descendant. Split
/// out from the per-node suffix (`layout_rect`) so a tree-shaped UI
/// avoids re-hashing the parent context on every node.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
struct CascadePrefixBytes {
    parent_transform: TranslateScale, // 12B
    clip_rect: Rect,                  // 16B (zeroed when absent)
    clip_present: u8,
    parent_dis: u8,
    parent_inv: u8,
    _pad: u8,
}

#[inline]
fn build_cascade_prefix(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
) -> Hasher {
    let (clip_rect, clip_present) = match parent_clip {
        Some(c) => (c, 1u8),
        None => (Rect::ZERO, 0u8),
    };
    let packed = CascadePrefixBytes {
        parent_transform,
        clip_rect,
        clip_present,
        parent_dis: parent_dis as u8,
        parent_inv: parent_inv as u8,
        _pad: 0,
    };
    let mut h = Hasher::new();
    h.pod(&packed);
    h
}

#[inline]
fn finish_cascade_input(prefix: &Hasher, layout_rect: Rect, invisible: bool) -> CascadeInputHash {
    let mut h = prefix.clone();
    h.pod(&layout_rect);
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
/// union to track exactly the set of pushed rows; doing both here makes
/// the two legs impossible to desync at a call site.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Option<Rect>, screen: Rect, hash: NodeHash) {
    *union = Some(union.map_or(screen, |a| a.union(screen)));
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
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    shape_clip: Option<Rect>,
    shape_transform: TranslateScale,
    clips: bool,
}

/// Emit every paint row for `node` (chrome at row 0 when present, then
/// direct shapes in record order) via [`push_paint`], write the
/// covering [`Span`] into `node_spans[node]`, and return the
/// screen-space union of every row — used locally as the
/// `subtree_paint_rects` seed for the encoder's cull. Damage recomputes
/// the same union from the `paint_arena` rows on demand (its cold
/// paths), so it isn't stored per node.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). The two transforms
/// are the only structural difference between the two row kinds.
///
/// # Invariant
///
/// The returned `Rect` is bit-identical to the screen-space union of
/// `arena.rows[paints_start..arena.rows.len()].iter().map(|p| p.screen)`
/// — the same union `damage::union_screens` recomputes from the stored
/// rows. [`push_paint`] keeps the union and the pushed rows in lockstep;
/// the chromeless clip-only branch is the sole fold-without-push case
/// (it contributes a cull rect but emits no pixels).
fn compute_paint_rect(ctx: PaintRectCtx<'_>, arena: &mut PaintArena) -> Rect {
    let PaintRectCtx {
        tree,
        layout,
        node,
        layout_rect,
        parent_transform,
        parent_clip,
        shape_clip,
        shape_transform,
        clips,
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
        let chrome_local = if bg.shadow.is_noop() {
            owner_local
        } else {
            let g = bg.shadow.geom();
            owner_local.union(shadow_paint_rect_local(
                None,
                layout_rect.size,
                g.offset,
                g.blur,
                g.spread,
                bg.shadow.inset(),
            ))
        };
        let screen = lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip);
        push_paint(arena, &mut union, screen, bg.hash);
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        let screen = lift_to_screen(owner_local, layout_rect.min, parent_transform, parent_clip);
        union = Some(union.map_or(screen, |a| a.union(screen)));
    }

    if tree.records.shape_span()[node.idx()].len > 0 {
        let text_span = layout.text_spans[node.idx()];
        let mut text_ord: u32 = 0;
        let shape_hashes = tree.shapes.hashes.as_slice();
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            let TreeItem::ShapeRecord(idx, s) = item else {
                continue;
            };
            // Text shapes live only on Leaf nodes (`leaf_text_shapes`
            // asserts the same), so when this node has any text shape
            // `text_span.len` must equal the count of `Text` variants
            // yielded by `TreeItems` here. Drift would silently fall
            // back to the owner rect — assert instead.
            let (local, text_measured) = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    assert!(
                        text_ord < text_span.len,
                        "cascade saw a text shape without a matching ShapedText entry — \
                         leaf_content_size and the cascade walk are out of sync",
                    );
                    let shaped = layout.text_shapes[(text_span.start + text_ord) as usize];
                    text_ord += 1;
                    let local = text_paint_bbox_local(
                        *local_origin,
                        *align,
                        tree.records.layout()[node.idx()].padding,
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
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union.unwrap_or(Rect::ZERO)
}

#[cfg(test)]
mod tests;
