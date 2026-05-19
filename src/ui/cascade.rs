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
use std::array;
use std::hash::Hasher as _;
use strum::EnumCount as _;

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
/// index columns into it. Grouped because the three vectors are
/// produced + cleared in lockstep by [`compute_paint_rect`]; keeping
/// them on one struct means there's exactly one place to call
/// `reset_for` from and the columns can't drift in length.
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

/// Read-only artifact of `CascadesEngine::run`. Holds the per-tree cascade
/// rows (indexed by `NodeId.0` within the matching tree) and a global
/// `WidgetId`-keyed hit index.
pub(crate) struct Cascades {
    /// Per-layer per-node cascade rows. Same indexing as
    /// `Tree::records`: `rows[layer.idx()][node.idx()]`.
    pub(crate) rows: [Vec<Cascade>; Layer::COUNT],
    /// Per-layer per-node subtree paint rect — `Cascade.paint_rect`
    /// rolled up with every descendant's `subtree_paint_rects[i]`.
    /// Stored parallel to `rows` (not inline on `Cascade`) so the
    /// damage diff's hot row scan stays cache-tight (it reads
    /// `paint_rect` + `cascade_input` only); the encoder is the sole
    /// reader and pays one indexed load per cull check. Computed
    /// inline in `run_tree` via a stack-frame accumulator. Read by
    /// the encoder for the viewport + damage subtree culls where
    /// "may I skip the whole subtree?" must consider overhanging
    /// descendants — Canvas-positioned children outside the parent's
    /// `Fixed` bound, shapes with negative-margin overhang, etc.
    /// Invisible subtrees seed with `Rect::ZERO` so a long-lived
    /// hidden subtree doesn't keep the cull from firing at ancestors.
    pub(crate) subtree_paint_rects: [Vec<Rect>; Layer::COUNT],
    /// Per-layer unified paint arena (rows + per-node spans + shape→paint
    /// translation). Three columns travel together — written in
    /// lockstep by `compute_paint_rect`, cleared together each frame.
    pub(crate) paint_arenas: [PaintArena; Layer::COUNT],
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
            rows: array::from_fn(|_| Vec::new()),
            subtree_paint_rects: array::from_fn(|_| Vec::new()),
            paint_arenas: array::from_fn(|_| PaintArena::default()),
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

    /// Borrow the per-tree cascade rows for `layer`.
    #[inline]
    pub(crate) fn rows_for(&self, layer: Layer) -> &[Cascade] {
        &self.rows[layer.idx()]
    }

    /// Borrow the per-tree subtree-paint-rect column for `layer`.
    /// Parallel to [`Self::rows_for`]; indexed by `NodeId.0` the
    /// same way.
    #[inline]
    pub(crate) fn subtree_paint_rects_for(&self, layer: Layer) -> &[Rect] {
        &self.subtree_paint_rects[layer.idx()]
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
            let i = layer.idx();
            let layer_layout = &layout.layers[i];
            let r = &mut layout.cascades;
            let n = tree.records.len();
            r.rows[i].clear();
            r.rows[i].reserve(n);
            r.subtree_paint_rects[i].clear();
            r.subtree_paint_rects[i].reserve(n);
            r.paint_arenas[i].reset_for(n, tree.shapes.records.len());
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
    let li = layer.idx();
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
            finalize_frame(stack, &mut cascades.subtree_paint_rects[li], popped);
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
            &mut cascades.paint_arenas[li],
        );
        // Invisible nodes never paint, so seeding their subtree
        // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
        // from inflating the ancestor's `subtree_paint_rect` (and
        // killing the encoder's viewport / damage cull at that
        // ancestor). Visibility is in `cascade_input` regardless, so
        // damage tracking is unaffected.
        let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
        cascades.rows[li].push(Cascade {
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
        cascades.subtree_paint_rects[li].push(subtree_seed);

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
        finalize_frame(stack, &mut cascades.subtree_paint_rects[li], popped);
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
mod tests {
    use crate::Ui;
    use crate::forest::Layer;
    use crate::forest::element::Configure;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::color::Color;
    use crate::primitives::corners::Corners;
    use crate::primitives::rect::Rect;
    use crate::primitives::stroke::Stroke;
    use crate::primitives::transform::TranslateScale;
    use crate::primitives::widget_id::WidgetId;
    use crate::shape::Shape;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};

    /// A direct shape recorded on a panel with `.transform(...)` must
    /// land in `Cascades::paint_arenas` at the *composed* transform
    /// (parent ∘ self), not just `parent_transform`. Pins the cascade
    /// half of the `Panel::transform`-applies-to-body contract — the
    /// encoder half is already pinned by
    /// `transformed_panel_applies_transform_to_direct_shapes`.
    #[test]
    fn shape_rect_composes_self_transform() {
        let scale = 3.0;
        let translate = Vec2::new(10.0, 20.0);
        let xform = TranslateScale::new(translate, scale);

        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::canvas()
                    .id(WidgetId::from_hash("xpanel"))
                    .size(Sizing::Fixed(300.0))
                    .transform(xform)
                    .show(ui, |ui| {
                        ui.add_shape(Shape::RoundedRect {
                            local_rect: Some(Rect::new(0.0, 0.0, 30.0, 30.0)),
                            radius: Corners::ZERO,
                            fill: Color::rgb(0.5, 0.5, 0.5).into(),
                            stroke: Stroke::ZERO,
                        });
                    });
            });
        });

        let layer_idx = Layer::Main.idx();
        let cascades = &ui.layout.cascades;
        let paint_idx = cascades.paint_arenas[layer_idx].shape_to_paint[0] as usize;
        let shape_rect = cascades.paint_arenas[layer_idx].rows[paint_idx].screen;
        // The Panel sits at the hstack origin (0, 0). Owner-local
        // shape rect is (0, 0, 30, 30); after `parent ∘ self`:
        //   min = (0, 0) * 3 + (10, 20) = (10, 20)
        //   size = (30, 30) * 3 = (90, 90)
        let eps = 1e-3;
        assert!(
            (shape_rect.min.x - 10.0).abs() < eps
                && (shape_rect.min.y - 20.0).abs() < eps
                && (shape_rect.size.w - 90.0).abs() < eps
                && (shape_rect.size.h - 90.0).abs() < eps,
            "expected shape_rect = (10, 20, 90, 90); got {shape_rect:?}",
        );
    }

    /// `.transform(zoom=S)` on an off-origin panel must anchor the
    /// scale at the panel's own `layout_rect.min`, not at the
    /// cascade's (0, 0). A child at panel-local (0, 0) should land
    /// at the panel's origin regardless of `S` — without anchoring it
    /// would slide off by `panel.min * (S - 1)`. Pins the cascade-
    /// level half of the "scale my body about my own origin"
    /// `Panel::transform` contract.
    #[test]
    fn self_transform_anchors_scale_at_panel_origin() {
        let zoom = 2.0;
        let xform = TranslateScale::from_scale(zoom);

        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            // Push the transformed panel off the surface origin with a
            // leading sibling — Spacer-style placeholder so the panel
            // sits at (sibling_width, 0) instead of (0, 0).
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("spacer"))
                    .size(Sizing::Fixed(50.0))
                    .show(ui, |_| {});
                Panel::canvas()
                    .id(WidgetId::from_hash("xpanel"))
                    .size(Sizing::Fixed(200.0))
                    .transform(xform)
                    .show(ui, |ui| {
                        ui.add_shape(Shape::RoundedRect {
                            // Panel-local (0, 0) — the natural top-left
                            // of the panel's body.
                            local_rect: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
                            radius: Corners::ZERO,
                            fill: Color::rgb(0.5, 0.5, 0.5).into(),
                            stroke: Stroke::ZERO,
                        });
                    });
            });
        });

        let layer_idx = Layer::Main.idx();
        let cascades = &ui.layout.cascades;
        let paint_idx = cascades.paint_arenas[layer_idx].shape_to_paint[0] as usize;
        let shape_rect = cascades.paint_arenas[layer_idx].rows[paint_idx].screen;
        // Panel sits at (50, 0). Shape's panel-local (0, 0) should
        // map to screen (50, 0) under the anchor — the panel's own
        // top-left is the fixed point of its scale. Size is
        // `panel-local size * zoom = 10 * 2 = 20`.
        //
        // Without anchoring, the raw `parent.compose(self).apply(panel.min)`
        // would give `(50, 0) * 2 = (100, 0)` — content slides 50px
        // right of where it belongs.
        let eps = 1e-3;
        assert!(
            (shape_rect.min.x - 50.0).abs() < eps && (shape_rect.min.y - 0.0).abs() < eps,
            "expected shape min = (50, 0); got {:?} — scale should anchor at panel.min, \
             not at cascade origin",
            shape_rect.min,
        );
        assert!(
            (shape_rect.size.w - 20.0).abs() < eps && (shape_rect.size.h - 20.0).abs() < eps,
            "expected size = (20, 20) (panel-local * zoom); got {:?}",
            shape_rect.size,
        );
    }

    /// A panel with chrome emits a Paint row at the start of its
    /// node's `node_spans` span; a chromeless panel emits an empty
    /// span.
    #[test]
    fn node_spans_populated_for_chrome_panels_only() {
        use crate::primitives::background::Background;

        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("chrome"))
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .background(Background {
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
                Panel::hstack()
                    .id(WidgetId::from_hash("bare"))
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .show(ui, |_| {});
            });
        });

        let li = Layer::Main.idx();
        let cascades = &ui.layout.cascades;
        let by_id = &cascades.by_id;
        let chrome_idx = by_id[&WidgetId::from_hash("chrome")] as usize;
        let bare_idx = by_id[&WidgetId::from_hash("bare")] as usize;
        let chrome_span = cascades.paint_arenas[li].node_spans[chrome_idx];
        let bare_span = cascades.paint_arenas[li].node_spans[bare_idx];

        assert!(
            chrome_span.len > 0
                && cascades.paint_arenas[li].rows[chrome_span.start as usize]
                    .screen
                    .area()
                    > 0.0,
            "chromed panel must have a non-empty paint span with non-zero chrome rect",
        );
        assert_eq!(
            bare_span.len, 0,
            "chromeless panel must have empty paint span; got {bare_span:?}",
        );
    }

    /// `node_spans` length matches the layer's node count so the
    /// damage diff can index by `NodeId.0` without a bounds-cap.
    #[test]
    fn node_spans_sized_to_node_count() {
        let mut ui = Ui::for_test();
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            Panel::hstack().auto_id().show(ui, |ui| {
                Panel::hstack().auto_id().show(ui, |_| {});
            });
        });
        let li = Layer::Main.idx();
        let nodes = ui.forest.tree(Layer::Main).records.len();
        assert_eq!(
            ui.layout.cascades.paint_arenas[li].node_spans.len(),
            nodes,
            "node_spans column must be sized to the layer's node count",
        );
    }
}
