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
use crate::forest::shapes::record::{ShapeRecord, shadow_paint_rect_local, text_paint_bbox_local};
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::input::sense::Sense;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{rect::Rect, transform::TranslateScale};
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
}

impl LayerCascades {
    /// Reset all per-node columns for `n_nodes` and `n_shapes`.
    /// `rows` and `subtree_paint_rects` are cleared and reserved
    /// (filled by per-node pushes during the walk); `paint_arena`
    /// columns reset according to their own sizing rules.
    pub(crate) fn reset_for(&mut self, n_nodes: usize, n_shapes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.subtree_paint_rects.clear();
        self.subtree_paint_rects.reserve(n_nodes);
        self.paint_arena.reset_for(n_nodes, n_shapes);
    }
}

/// Read-only artifact of `CascadesEngine::run`. Holds per-layer
/// cascade state (per-node rows, subtree rollups, paint arena — see
/// [`LayerCascades`]) and a global `WidgetId`-keyed hit index.
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
    pub(crate) by_id: FxHashMap<WidgetId, u32>,
}

impl Default for Cascades {
    fn default() -> Self {
        Self {
            layers: PerLayer::default(),
            entries: Soa::new(),
            by_id: FxHashMap::default(),
        }
    }
}

impl Cascades {
    /// Push a hit-test row and register its entry index in `by_id`.
    /// One source of truth for "append to the hit index"; callers
    /// can't drift a parallel array out of sync because there isn't
    /// one any more — the SoA storage keeps every column lockstep.
    #[inline]
    fn push_entry(&mut self, row: EntryRow) {
        self.by_id.insert(row.widget_id, self.entries.len() as u32);
        self.entries.push(row);
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
            r.by_id.clear();
            r.by_id.reserve(total);
        }

        for (layer, tree) in forest.iter_paint_order() {
            let layer_layout = &layout.layers[layer];
            let r = &mut layout.cascades;
            let n = tree.records.len();
            r.layers[layer].reset_for(n, tree.shapes.records.len());
            self.stack.clear();
            run_tree(tree, layer_layout, r, layer, &mut self.stack);
        }
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
        let (parent_transform, parent_clip, parent_dis, parent_inv) = match stack.last() {
            Some(p) => (p.transform, p.clip, p.disabled, p.invisible),
            None => (TranslateScale::IDENTITY, None, false, false),
        };

        let id = NodeId(i as u32);
        let attrs = attrs_col[i];

        let disabled = parent_dis || attrs.is_disabled();
        let invisible = parent_inv || !layout_col[i].visibility().is_visible();

        let layout_rect = layout.rect[id.idx()];
        let screen_rect = parent_transform.apply_rect(layout_rect);
        let visible_rect = clip_to(screen_rect, parent_clip);
        let paint_rect = compute_paint_rect(
            tree,
            layout,
            id,
            layout_rect,
            parent_transform,
            parent_clip,
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
            cascade_input: hash_cascade_input(
                parent_transform,
                parent_clip,
                parent_dis,
                parent_inv,
                layout_rect,
                invisible,
            ),
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
        let node_transform = tree.transform_of(id);
        let desc_transform = match node_transform {
            Some(t) => parent_transform.compose(t.anchored_at(layout_rect.min)),
            None => parent_transform,
        };
        let desc_clip = if attrs.clip_mode().is_clip() {
            Some(clip_to(screen_rect, parent_clip))
        } else {
            parent_clip
        };
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

        stack.push(Frame {
            transform: desc_transform,
            clip: desc_clip,
            disabled,
            invisible,
            subtree_end: ends[i],
            node_idx: i,
            subtree_paint_rect: subtree_seed,
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

#[inline]
fn hash_cascade_input(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
    layout_rect: Rect,
    invisible: bool,
) -> CascadeInputHash {
    let (clip_rect, clip_present) = match parent_clip {
        Some(c) => (c, 1u8),
        None => (Rect::ZERO, 0u8),
    };
    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::NoUninit)]
    struct CascadeInputBytes {
        parent_transform: TranslateScale, // 12B
        layout_rect: Rect,                // 16B
        clip_rect: Rect,                  // 16B (zeroed when absent)
        clip_present: u8,
        parent_dis: u8,
        parent_inv: u8,
        _pad: u8,
    }
    let packed = CascadeInputBytes {
        parent_transform,
        layout_rect,
        clip_rect,
        clip_present,
        parent_dis: parent_dis as u8,
        parent_inv: parent_inv as u8,
        _pad: 0,
    };

    let mut h = Hasher::new();
    h.pod(&packed);
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
fn compute_paint_rect(
    tree: &Tree,
    layout: &LayerLayout,
    node: NodeId,
    layout_rect: Rect,
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    arena: &mut PaintArena,
) -> Rect {
    let paints_start = arena.rows.len() as u32;
    let self_transform = tree
        .transform_of(node)
        .map(|t| t.anchored_at(layout_rect.min))
        .unwrap_or(TranslateScale::IDENTITY);
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
    } else if tree.records.attrs()[node.idx()].clip_mode().is_clip() {
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
            let local = match s {
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
                    text_paint_bbox_local(
                        layout_rect.size,
                        tree.records.layout()[node.idx()].padding,
                        *local_origin,
                        shaped.measured,
                        *align,
                    )
                }
                _ => s.paint_bbox_local(layout_rect.size),
            };
            let screen = lift_to_screen(local, layout_rect.min, shape_transform, parent_clip);
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
