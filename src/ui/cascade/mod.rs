//! Per-frame post-arrange state.
//!
//! `CascadesEngine` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Forest, &Layout)` and produces
//! a fresh `Cascades` — per-tree per-node cascade rows plus a
//! global hit index, all populated in a single per-tree pre-order walk.
//! Downstream phases (damage diff, input hit-test, renderer encoder)
//! take `&Cascades` as their single frozen-state handle.

mod cascade_input;
mod paint_rect;
mod reuse;
mod walk;

use crate::forest::Forest;
use crate::forest::per_layer::PerLayer;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::seen_ids::{Endpoint, WidgetIdMap};
use crate::input::sense::Sense;
use crate::layout::Layout;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use soa_rs::{Soa, Soars};

use reuse::{CascadeSnapshot, structure_matches};
use walk::{Frame, run_pass};
#[cfg(test)]
use walk::{WalkStats, cross_check};

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
    pub(crate) fn push_entry(&mut self, row: EntryRow) {
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

#[cfg(test)]
mod tests;
