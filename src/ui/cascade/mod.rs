//! Per-frame post-arrange state.
//!
//! `CascadesEngine` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Forest, &Layout)` and produces
//! a fresh `Cascades` — per-tree per-node cascade rows plus a
//! global hit index, all populated in a single per-tree pre-order walk.
//! Downstream phases (damage diff, input hit-test, renderer encoder)
//! take `&Cascades` as their single frozen-state handle.

use crate::common::hash::Hasher;
use crate::common::per_layer::PerLayer;
use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
use crate::forest::seen_ids::Endpoint;
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
use rustc_hash::FxHashMap;
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

/// Per-layer paint state: the unified [`Paint`] arena plus the two
/// index columns into it. The three vectors have different lengths
/// (per-row, per-node, per-shape) but share a lifecycle — written
/// during [`compute_paint_rect`], reset together each frame in
/// [`Self::reset_for`].
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
    /// `shape_idx → paint_idx` translation. Lets the paint-anim damage
    /// path recover a shape's screen rect via
    /// `rows[shape_to_paint[shape_idx] as usize].screen`. `u32::MAX`
    /// for shapes never visited by the cascade walk.
    pub(crate) shape_to_paint: Vec<u32>,
}

impl PaintArena {
    /// Reset all three columns for a new frame. `n_nodes` sizes
    /// `node_spans` (zero-init to `Span::default()`); `n_shapes` sizes
    /// `shape_to_paint` (zero-init to `u32::MAX`); `rows` is cleared
    /// and reserved for the expected upper bound.
    pub(crate) fn reset_for(&mut self, n_nodes: usize, n_shapes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.clear();
        self.node_spans.resize(n_nodes, Span::default());
        self.shape_to_paint.clear();
        self.shape_to_paint.resize(n_shapes, u32::MAX);
    }
}

/// Per-node cascade row: what the encoder and damage diff need to
/// know about node `i` after ancestor state has been folded in.
/// Ancestor `transform` and `clip` themselves never leave `run_tree`
/// — they live on its stack `Frame` and are baked into `paint_rect`
/// before publishing.
///
/// Packed to 24 bytes (16 for `paint_rect`, 8 for the
/// fingerprint-and-`invisible` u64). The encoder reads `invisible`
/// via `cascade_input.invisible()`; damage compares the full u64.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    /// **Own** paint extent: screen-space union of every [`Paint`] row
    /// emitted for this node by [`compute_paint_rect`] — chrome
    /// (inflated by drop-shadow halo when present) plus each direct
    /// shape's tight bbox, all post-transform and clipped to the
    /// ancestor clip. Used by [`crate::ui::damage::DamageEngine`] as
    /// the per-widget paint snapshot; keeping it tight (no descendant
    /// rollup) lets a leaf colour change produce a leaf-sized dirty
    /// rect instead of an ancestor-sized one. Hit-test uses its own
    /// `EntryRow.rect`; shadows aren't clickable.
    pub(crate) paint_rect: Rect,
    /// Fingerprint of the ancestor state + own arranged rect that
    /// flowed into this row, packed with the cascade-resolved
    /// `invisible` bit in the high position. Paired with
    /// `Tree.rollups.subtree[i]` to drive damage's subtree-skip fast
    /// path; read by the encoder via `cascade_input.invisible()`.
    pub(crate) cascade_input: CascadeInputHash,
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
    /// ancestor prefix per node. See `hash_cascade_input`.
    cascade_prefix: Hasher,
}

/// All per-layer cascade state grouped on one struct. `rows` +
/// `subtree_paint_rects` + `paint_arena` are produced together in a
/// single [`run_tree`] pass, reset together at frame start, and read
/// together by the damage diff and encoder — keeping them on one
/// struct means there's exactly one indexing point per layer and no
/// chance of resetting one column but not another.
///
/// ## AoS vs SoA split
///
/// The per-node data is deliberately divided three ways, driven by
/// who reads what together:
///
/// - [`Cascade`] (`paint_rect` + `cascade_input`) is **AoS**: damage's
///   hot per-node scan reads both fields per iteration, so colocating
///   them keeps the inner loop at 24 B/node and one indexed load.
/// - [`Self::subtree_paint_rects`] is **split out**: read by the
///   encoder cull but *not* by damage. Inlining it into `Cascade`
///   would widen damage's per-row footprint by 16 B (~66 %) for a
///   read it doesn't perform.
/// - [`Self::paint_arena`] holds per-paint-row data (chrome + per-shape
///   [`Paint`]s, the `node_spans` index, and the `shape_to_paint`
///   reverse map). Read only on cache-miss paths (vacant insert, hash
///   mismatch, paint-anim lookup), so it sits behind a `node_spans[i]`
///   indirection that damage's fast path skips entirely.
///
/// Any new per-node datum that damage's hot scan needs to read every
/// frame belongs inline on `Cascade`; anything read only by the
/// encoder or only on cache-miss paths should stay parallel.
#[derive(Default)]
pub(crate) struct LayerCascades {
    /// Per-node cascade rows, indexed the same way as
    /// `Tree::records`: `rows[node.idx()]`.
    pub(crate) rows: Vec<Cascade>,
    /// Per-node subtree paint rect — [`Cascade::paint_rect`] rolled up
    /// with every descendant's `subtree_paint_rects[i]`. Stored
    /// alongside `rows` (not inline on `Cascade`) so the damage
    /// diff's hot row scan stays cache-tight (reads `paint_rect` +
    /// `cascade_input` only — 24 B/node); the encoder is the sole
    /// reader and pays one indexed load per cull check. Computed
    /// inline in [`run_tree`] via a stack-frame accumulator. Read by
    /// the encoder for the viewport + damage subtree culls where
    /// "may I skip the whole subtree?" must consider overhanging
    /// descendants — Canvas-positioned children outside the parent's
    /// `Fixed` bound, shapes with negative-margin overhang, etc.
    /// Invisible subtrees seed with `Rect::ZERO` so a long-lived
    /// hidden subtree doesn't keep the cull from firing at ancestors.
    pub(crate) subtree_paint_rects: Vec<Rect>,
    /// Unified paint arena (rows + per-node spans + shape→paint
    /// translation).
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
    /// Reset all per-node columns for `n_nodes` and `n_shapes` and
    /// stamp the layer's `entries_base` in one call — both prep this
    /// layer for the upcoming `run_tree`, splitting them invites a
    /// caller that resets but forgets the offset (or vice versa).
    /// `rows` and `subtree_paint_rects` are cleared and reserved
    /// (filled by per-node pushes during the walk); `paint_arena`
    /// columns reset according to their own sizing rules.
    pub(crate) fn reset_for(&mut self, n_nodes: usize, n_shapes: usize, entries_base: u32) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.subtree_paint_rects.clear();
        self.subtree_paint_rects.reserve(n_nodes);
        self.paint_arena.reset_for(n_nodes, n_shapes);
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
    pub(crate) by_id: FxHashMap<WidgetId, Endpoint>,
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
}

impl Cascades {
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
}

impl CascadesEngine {
    /// Walk every tree in paint order; produce one `Cascade` row per
    /// node in each tree's slot, and append a global hit entry per
    /// node. Writes into `layout.cascades`. Anchor offset for each
    /// layer is read from the layer's own `RootSlot.anchor` — no
    /// parent transform plumbing is needed because trees never share
    /// NodeId space.
    #[profiling::function]
    pub(crate) fn run(&mut self, forest: &Forest, layout: &mut Layout) {
        let total: usize = forest.trees.iter().map(|t| t.records.len()).sum();
        {
            let r = &mut layout.cascades;
            r.entries.clear();
            r.entries.reserve(total);
        }

        for (layer, tree) in forest.iter_paint_order() {
            let layer_layout = &layout.layers[layer];
            let r = &mut layout.cascades;
            let n = tree.records.len();
            let entries_base = r.entries.len() as u32;
            r.layers[layer].reset_for(n, tree.shapes.records.len(), entries_base);
            self.stack.clear();
            run_tree(tree, layer_layout, r, layer, &mut self.stack);
            // Invariant guarding `Cascades::entry_idx_of`'s
            // `entries_base + node.0` arithmetic: every node in
            // `tree.records` must push exactly one `EntryRow`. An
            // early-continue / skip-invisible optimization inside
            // `run_tree` that doesn't push would silently shift every
            // later widget's entry by one. Release `assert!` —
            // `n + entries_base` is already loaded, the equality is a
            // single compare.
            assert_eq!(
                r.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree pushed {} entries for layer with {n} nodes — every record must push exactly one row to keep entries_base + node.0 valid",
                r.entries.len() as u32 - entries_base,
            );
        }

        // Snapshot `seen.curr` for inter-pass `response_for` lookups.
        // `request_relayout`'s second pass clears `curr` in
        // `pre_record` *before* the second pass's widgets call
        // `response_for(id)`, so the data has to live on `Cascades`
        // instead. `clone_from` reuses storage — one O(N) memcpy
        // replaces N per-widget hashmap inserts.
        layout.cascades.by_id.clone_from(&forest.ids.curr);
    }
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

fn run_tree(
    tree: &Tree,
    layout: &LayerLayout,
    cascades: &mut Cascades,
    layer: Layer,
    stack: &mut Vec<Frame>,
) {
    let n = tree.records.len();
    let layout_col = tree.records.layout();
    let attrs_col = tree.records.attrs();
    let widget_ids = tree.records.widget_id();
    let ends = tree.records.subtree_end();
    let root_prefix = build_cascade_prefix(TranslateScale::IDENTITY, None, false, false);

    for i in 0..n {
        while let Some(top) = stack.last() {
            if (i as u32) < top.subtree_end {
                break;
            }
            let popped = stack.pop().unwrap();
            finalize_frame(
                stack,
                &mut cascades.layers[layer].subtree_paint_rects,
                popped,
            );
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

        let id = NodeId(i as u32);
        let attrs = attrs_col[i];

        let disabled = parent_dis || attrs.is_disabled();
        let invisible = parent_inv || !layout_col[i].visibility().is_visible();

        let layout_rect = layout.rect[id.idx()];
        let screen_rect = parent_transform.apply_rect(layout_rect);
        let visible_rect = clip_to(screen_rect, parent_clip);
        // Self-transform is read once here and threaded into both
        // descendant transform composition (below) and
        // `compute_paint_rect`'s shape-transform composition —
        // `tree.transform_of` is a sparse-column probe, and doing it
        // twice per node showed up in the cascade self-time profile.
        let node_transform = tree.transform_of(id);
        let self_transform = node_transform
            .map(|t| t.anchored_at(layout_rect.min))
            .unwrap_or(TranslateScale::IDENTITY);
        let clips = attrs.clip_mode().is_clip();
        // Encoder's clip mask is `rect.deflated_by(padding)`, pushed
        // **before** the body. Direct shapes and descendants both
        // paint inside it. Mirror that here so per-shape damage rects
        // and inherited child clips reflect what actually paints —
        // otherwise a TextEdit's tall text shape (extent = full
        // shaped buffer) reports damage well past the editor's rect
        // on every scroll tick.
        let shape_clip = if clips {
            let padding = layout_col[i].padding;
            let mask_local = layout_rect.deflated_by(padding);
            Some(clip_to(
                parent_transform.apply_rect(mask_local),
                parent_clip,
            ))
        } else {
            parent_clip
        };
        let paint_rect = compute_paint_rect(
            tree,
            layout,
            id,
            layout_rect,
            parent_transform,
            parent_clip,
            shape_clip,
            self_transform,
            clips,
            &mut cascades.layers[layer].paint_arena,
        );
        // Invisible nodes never paint, so seeding their subtree
        // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
        // from inflating the ancestor's `subtree_paint_rect` (and
        // killing the encoder's viewport / damage cull at that
        // ancestor). Visibility is in `cascade_input` regardless, so
        // damage tracking is unaffected.
        let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
        cascades.layers[layer].rows.push(Cascade {
            paint_rect,
            cascade_input: finish_cascade_input(parent_prefix, layout_rect, invisible),
        });
        cascades.layers[layer]
            .subtree_paint_rects
            .push(subtree_seed);

        // `Panel::transform` semantics: scale pivots about the node's
        // own `layout_rect.min`, not the cascade's (0, 0). The
        // anchoring cancels the `panel.min * (1 - scale)` drift that
        // a raw `self.compose` against absolute-coord layout rects
        // would introduce. Identity-preserving — no-op when
        // `scale == 1`. See `TranslateScale::anchored_at`.
        // `self_transform` already incorporates the anchoring above;
        // for descendants we compose it onto the parent's transform.
        // When `node_transform` is `None`, `self_transform` is
        // `IDENTITY` and `compose` would yield the same result,
        // but skip the 3×mul + 3×add anyway — most nodes have no
        // transform, so the early-out is the steady-state path.
        let desc_transform = match node_transform {
            Some(_) => parent_transform.compose(self_transform),
            None => parent_transform,
        };
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
        cascades.push_entry(EntryRow {
            widget_id: widget_ids[i],
            rect: visible_rect,
            sense,
            focusable,
            disabled,
            layout_rect,
        });

        // Leaves can't be a parent_prefix for anyone — skip the 32 B
        // prefix-hash work, push a fresh-state Hasher as a placeholder.
        // `Hasher::new()` is just `FxHasher { hash: 0 }`, ~free.
        let subtree_end = ends[i];
        let is_leaf = subtree_end == (i as u32) + 1;
        let cascade_prefix = if is_leaf {
            Hasher::new()
        } else {
            build_cascade_prefix(desc_transform, desc_clip, disabled, invisible)
        };
        stack.push(Frame {
            transform: desc_transform,
            clip: desc_clip,
            disabled,
            invisible,
            subtree_end,
            node_idx: i,
            subtree_paint_rect: subtree_seed,
            cascade_prefix,
        });
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

#[inline]
fn clip_to(rect: Rect, clip: Option<Rect>) -> Rect {
    match clip {
        Some(c) => rect.intersect(c),
        None => rect,
    }
}

/// Lift an owner-local rect into screen space: translate by the owner's
/// arranged origin, apply the relevant transform (`parent_transform`
/// for chrome / clip lift, `shape_transform` for shapes), then clip
/// to the ancestor clip. One source of truth for the three coord-
/// space hops the paint emit does.
#[inline]
fn lift_to_screen(local: Rect, origin: Vec2, t: TranslateScale, clip: Option<Rect>) -> Rect {
    clip_to(
        t.apply_rect(Rect {
            min: origin + local.min,
            size: local.size,
        }),
        clip,
    )
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

#[inline]
fn union_in(acc: &mut Option<Rect>, screen: Rect) {
    *acc = Some(match *acc {
        Some(a) => a.union(screen),
        None => screen,
    });
}

/// Emit every paint row for `node` (chrome at row 0 when present,
/// then direct shapes in record order), stamp each shape's paint
/// index into `shape_to_paint` for the paint-anim reverse lookup,
/// write the covering [`Span`] into `node_spans[node]`, and return
/// the screen-space union of every row — fed into `Cascade.paint_rect`
/// for the damage diff and rolled into `subtree_paint_rects` for the
/// encoder's cull.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). The two transforms
/// are the only structural difference between the two row kinds —
/// both flow through [`push_row`].
///
/// # Invariant
///
/// The returned `Rect` is bit-identical to the screen-space union of
/// `arena.rows[paints_start..arena.rows.len()].iter().map(|p| p.screen)`.
/// `Cascade.paint_rect` stores it as a precomputed scalar so damage's
/// hot per-node scan reads `rows[i].paint_rect` in one load instead of
/// looping over each node's Paint slice to recompute the union.
/// Touching the union accumulator without also updating the per-paint
/// `screen` (or vice versa) breaks the damage fast path silently —
/// keep both legs in lockstep when adding new paint contributions.
#[allow(clippy::too_many_arguments)]
fn compute_paint_rect(
    tree: &Tree,
    layout: &LayerLayout,
    node: NodeId,
    layout_rect: Rect,
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    shape_clip: Option<Rect>,
    // `self_transform` and `clips` are computed once in `run_tree`
    // and threaded in to avoid re-probing the sparse `transform_of`
    // column and the SoA `attrs` column for the same node here —
    // both showed up as duplicate work in cascade profiling.
    self_transform: TranslateScale,
    clips: bool,
    arena: &mut PaintArena,
) -> Rect {
    let paints_start = arena.rows.len() as u32;
    let shape_transform = parent_transform.compose(self_transform);

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
        union_in(&mut union, screen);
        arena.rows.push(Paint {
            screen,
            hash: bg.hash,
        });
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        let screen = lift_to_screen(owner_local, layout_rect.min, parent_transform, parent_clip);
        union_in(&mut union, screen);
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
            union_in(&mut union, screen);
            arena.shape_to_paint[idx as usize] = arena.rows.len() as u32;
            arena.rows.push(Paint {
                screen,
                hash: shape_hashes[idx as usize],
            });
        }
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union.unwrap_or(Rect::ZERO)
}

#[cfg(test)]
mod tests;
