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
use crate::forest::rollups::CascadeInputHash;
use crate::forest::shapes::record::shadow_paint_rect_local;
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::input::sense::Sense;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{rect::Rect, transform::TranslateScale};
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::array;
use std::hash::Hasher as _;
use strum::EnumCount as _;

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
    /// **Own** paint extent: the node's layout rect transformed into
    /// screen space, unioned with the owner-local
    /// [`Overhang`](crate::forest::shapes::record::Overhang) of each
    /// *direct* shape (drop-shadow halos today), then clipped to the
    /// ancestor clip. Used by [`crate::ui::damage::DamageEngine`] as
    /// the per-widget paint snapshot — keeping it tight (no
    /// descendant rollup) lets a leaf colour change produce a leaf-
    /// sized dirty rect instead of an ancestor-sized one. Hit-test
    /// uses its own `HitEntry.rect`; shadows aren't clickable.
    pub(crate) paint_rect: Rect,
    /// Fingerprint of the ancestor state + own arranged rect that
    /// flowed into this row, packed with the cascade-resolved
    /// `invisible` bit in the high position. Paired with
    /// `Tree.rollups.subtree[i]` to drive damage's subtree-skip fast
    /// path; read by the encoder via `cascade_input.invisible()`.
    pub(crate) cascade_input: CascadeInputHash,
}

/// 20 B per row, align 4. `WidgetId` is split into the parallel
/// `Cascades::entry_ids` so the hot reverse-scan loop in `hit_test*`
/// only touches `rect` + the small flags — the `u64` id is loaded
/// once on match, not on every reject.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitEntry {
    pub(crate) rect: Rect,
    pub(crate) sense: Sense,
    pub(crate) focusable: bool,
    /// Effective disabled (self OR any ancestor). Mirrors what
    /// `cascaded_off` already used to null `sense`/`focusable`,
    /// preserved here so per-widget responses can read it.
    pub(crate) disabled: bool,
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
    /// `Tree::records`: `rows[layer as usize][node.index()]`.
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
    /// One [`Rect`] per shape in `tree.shapes.records`, per layer —
    /// `shape_rects[L][shape_idx]` is the screen-space damage bound
    /// for that shape. Written during the cascade walk in
    /// [`compute_paint_rect`] (same `TreeItems` pass that unions for
    /// `paint_rect`), so cascade stays a pure `&Forest → Cascades`
    /// producer. Indexed by `shape_idx` directly — same key as
    /// `tree.paint_anims.by_shape`, so callers (paint-anim damage
    /// today; future per-shape culling / debug) reach a shape's
    /// rect with one indexed load. `Rect::ZERO` for shapes never
    /// visited by the cascade walk (collapsed / invisible subtrees),
    /// keeping the column dense without a sentinel.
    pub(crate) shape_rects: [Vec<Rect>; Layer::COUNT],
    /// Pre-order rect/sense snapshot in the form hit-testing needs.
    /// Layers append in paint order so reverse iteration yields
    /// topmost-first.
    pub(crate) entries: Vec<HitEntry>,
    /// Parallel to `entries`: the `WidgetId` for each row, split out
    /// to keep the hot reverse-scan struct at 20 B (instead of 32 B
    /// with the `u64` id inline).
    pub(crate) entry_ids: Vec<WidgetId>,
    /// Parallel to `entries`: the widget's pre-transform layout rect
    /// (unclipped, in world coords). Surfaced via
    /// `ResponseState::layout_rect` so callers can read a widget's
    /// arranged position without the cascade's transform + clip
    /// applied — useful for drawing connection geometry into a
    /// scrolling/zoomed parent's coordinate system.
    pub(crate) entry_layout_rects: Vec<Rect>,
    pub(crate) by_id: FxHashMap<WidgetId, u32>,
}

impl Default for Cascades {
    fn default() -> Self {
        Self {
            rows: array::from_fn(|_| Vec::new()),
            subtree_paint_rects: array::from_fn(|_| Vec::new()),
            shape_rects: array::from_fn(|_| Vec::new()),
            entries: Vec::new(),
            entry_ids: Vec::new(),
            entry_layout_rects: Vec::new(),
            by_id: FxHashMap::default(),
        }
    }
}

impl Cascades {
    /// Lockstep push for the three parallel hit-index arrays —
    /// callers can't forget one and end up with `entries.len() !=
    /// entry_ids.len()`. Also updates `by_id` so the entry index is
    /// reachable by `WidgetId` lookup.
    fn push_entry(&mut self, wid: WidgetId, entry: HitEntry, layout_rect: Rect) {
        self.by_id.insert(wid, self.entries.len() as u32);
        self.entry_ids.push(wid);
        self.entry_layout_rects.push(layout_rect);
        self.entries.push(entry);
    }
}

impl Cascades {
    /// Reverse-iter entries → topmost-first under pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for
    /// hover, clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        for (i, e) in self.entries.iter().enumerate().rev() {
            if filter(e.sense) && e.rect.contains(pos) {
                return Some(self.entry_ids[i]);
            }
        }
        None
    }

    /// One reverse walk that finds the topmost match for both filters
    /// at once. Used on `PointerMoved` to recompute hover + scroll
    /// target without a second pass over `entries`.
    pub(crate) fn hit_test_pair(
        &self,
        pos: Vec2,
        a_filter: impl Fn(Sense) -> bool,
        b_filter: impl Fn(Sense) -> bool,
    ) -> HitPair {
        let mut a = None;
        let mut b = None;
        for (i, e) in self.entries.iter().enumerate().rev() {
            if !e.rect.contains(pos) {
                continue;
            }
            if a.is_none() && a_filter(e.sense) {
                a = Some(self.entry_ids[i]);
            }
            if b.is_none() && b_filter(e.sense) {
                b = Some(self.entry_ids[i]);
            }
            if a.is_some() && b.is_some() {
                break;
            }
        }
        HitPair {
            hover: a,
            scroll: b,
        }
    }

    pub(crate) fn hit_test_focusable(&self, pos: Vec2) -> Option<WidgetId> {
        for (i, e) in self.entries.iter().enumerate().rev() {
            if e.focusable && e.rect.contains(pos) {
                return Some(self.entry_ids[i]);
            }
        }
        None
    }

    /// Borrow the per-tree cascade rows for `layer`.
    #[inline]
    pub(crate) fn rows_for(&self, layer: Layer) -> &[Cascade] {
        &self.rows[layer as usize]
    }

    /// Borrow the per-tree subtree-paint-rect column for `layer`.
    /// Parallel to [`Self::rows_for`]; indexed by `NodeId.0` the
    /// same way.
    #[inline]
    pub(crate) fn subtree_paint_rects_for(&self, layer: Layer) -> &[Rect] {
        &self.subtree_paint_rects[layer as usize]
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct HitPair {
    pub(crate) hover: Option<WidgetId>,
    pub(crate) scroll: Option<WidgetId>,
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
            r.entry_ids.clear();
            r.entry_ids.reserve(total);
            r.entry_layout_rects.clear();
            r.entry_layout_rects.reserve(total);
            r.by_id.clear();
            r.by_id.reserve(total);
        }

        for (layer, tree) in forest.iter_paint_order() {
            let i = layer as usize;
            let layer_layout = &layout.layers[i];
            let r = &mut layout.cascades;
            let n = tree.records.len();
            r.rows[i].clear();
            r.rows[i].reserve(n);
            r.subtree_paint_rects[i].clear();
            r.subtree_paint_rects[i].reserve(n);
            let shape_rects = &mut r.shape_rects[i];
            shape_rects.clear();
            // Index-by-`shape_idx`. Resize so collapsed / invisible
            // subtrees (which `compute_paint_rect` skips writing for)
            // leave `Default::default()` in place — readers see zero,
            // which damage / culling treat as "contributes nothing".
            shape_rects.resize(tree.shapes.records.len(), Rect::ZERO);
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
    let li = layer as usize;
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

        let layout_rect = layout.rect[id.index()];
        let screen_rect = parent_transform.apply_rect(layout_rect);
        let visible_rect = clip_to(screen_rect, parent_clip);
        let paint_rect = compute_paint_rect(
            tree,
            id,
            layout_rect,
            parent_transform,
            parent_clip,
            &mut cascades.shape_rects[li],
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

        let node_transform = tree.transform_of(id);
        let desc_transform = match node_transform {
            Some(t) => parent_transform.compose(t),
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
        cascades.push_entry(
            widget_ids[i],
            HitEntry {
                rect: visible_rect,
                sense,
                focusable,
                disabled,
            },
            layout_rect,
        );

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

/// Union the owner-local `paint_bbox` of every direct shape on
/// `node` with the node's own rect, translate to tree-local coords,
/// apply `parent_transform`, then clip to the ancestor clip. Nodes
/// with no shapes — or with shapes whose bbox stays inside the
/// owner rect — fall through to the un-inflated path.
fn compute_paint_rect(
    tree: &Tree,
    node: NodeId,
    layout_rect: Rect,
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    shape_rects: &mut [Rect],
) -> Rect {
    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };
    let mut paint_local = owner_local;
    if tree.records.shape_span()[node.index()].len > 0 {
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            if let TreeItem::ShapeRecord(idx, s) = item {
                let bbox = s.paint_bbox_local(layout_rect.size);
                paint_local = paint_local.union(bbox);
                let tree_local = Rect {
                    min: layout_rect.min + bbox.min,
                    size: bbox.size,
                };
                let screen = clip_to(parent_transform.apply_rect(tree_local), parent_clip);
                shape_rects[idx as usize] = screen;
            }
        }
    }
    // Chrome-attached drop shadow inflates the same way a
    // `ShapeRecord::Shadow` would; encoder mirrors this via
    // `shadow_paint_rect_local` so paint extent and damage extent
    // stay in lockstep.
    if let Some(bg) = tree.chrome(node)
        && !bg.shadow.is_noop()
    {
        let s = &bg.shadow;
        let g = s.geom();
        let shadow_local = shadow_paint_rect_local(
            None,
            layout_rect.size,
            g.offset,
            g.blur,
            g.spread,
            s.inset(),
        );
        paint_local = paint_local.union(shadow_local);
    }
    let paint_tree_local = Rect {
        min: layout_rect.min + paint_local.min,
        size: paint_local.size,
    };
    clip_to(parent_transform.apply_rect(paint_tree_local), parent_clip)
}
