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
use crate::forest::rollups::CascadeInputHash;
use crate::forest::shapes::record::shadow_paint_rect_local;
use crate::forest::tree::{Layer, NodeId, Tree, TreeItem, TreeItems};
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
    /// Layout rect transformed into screen space, inflated by every
    /// shape's owner-local
    /// [`Overhang`](crate::forest::shapes::record::Overhang) (drop
    /// shadows are the only contributor today), then intersected with
    /// the ancestor clip. Drives both subtree culling (viewport +
    /// damage region intersection in the encoder) and damage tracking
    /// — so a tab swap clears the full shadow halo and a halo-only
    /// dirty patch still reaches the affected subtree. Hit-test uses
    /// its own `HitEntry.rect` (the un-inflated visible rect) —
    /// shadows aren't clickable.
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
}

/// Read-only artifact of `CascadesEngine::run`. Holds the per-tree cascade
/// rows (indexed by `NodeId.0` within the matching tree) and a global
/// `WidgetId`-keyed hit index.
pub(crate) struct Cascades {
    /// Per-layer per-node cascade rows. Same indexing as
    /// `Tree::records`: `rows[layer as usize][node.index()]`.
    pub(crate) rows: [Vec<Cascade>; Layer::COUNT],
    /// Pre-order rect/sense snapshot in the form hit-testing needs.
    /// Layers append in paint order so reverse iteration yields
    /// topmost-first.
    pub(crate) entries: Vec<HitEntry>,
    /// Parallel to `entries`: the `WidgetId` for each row, split out
    /// to keep the hot reverse-scan struct at 20 B (instead of 32 B
    /// with the `u64` id inline).
    pub(crate) entry_ids: Vec<WidgetId>,
    pub(crate) by_id: FxHashMap<WidgetId, u32>,
}

impl Default for Cascades {
    fn default() -> Self {
        Self {
            rows: array::from_fn(|_| Vec::new()),
            entries: Vec::new(),
            entry_ids: Vec::new(),
            by_id: FxHashMap::default(),
        }
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
            r.by_id.clear();
            r.by_id.reserve(total);
        }

        for (layer, tree) in forest.iter_paint_order() {
            let i = layer as usize;
            let layer_layout = &layout.layers[i];
            let r = &mut layout.cascades;
            let rows = &mut r.rows[i];
            rows.clear();
            rows.reserve(tree.records.len());
            self.stack.clear();
            run_tree(
                tree,
                layer_layout,
                rows,
                &mut r.entries,
                &mut r.entry_ids,
                &mut r.by_id,
                &mut self.stack,
            );
        }
    }
}

fn run_tree(
    tree: &Tree,
    layout: &LayerLayout,
    rows: &mut Vec<Cascade>,
    entries: &mut Vec<HitEntry>,
    entry_ids: &mut Vec<WidgetId>,
    by_id: &mut FxHashMap<WidgetId, u32>,
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
            stack.pop();
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
        let paint_rect = compute_paint_rect(tree, id, layout_rect, parent_transform, parent_clip);
        let row = Cascade {
            paint_rect,
            cascade_input: hash_cascade_input(
                parent_transform,
                parent_clip,
                parent_dis,
                parent_inv,
                layout_rect,
                invisible,
            ),
        };

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
        let widget_id = widget_ids[i];
        by_id.insert(widget_id, entries.len() as u32);
        entry_ids.push(widget_id);
        entries.push(HitEntry {
            rect: visible_rect,
            sense,
            focusable,
            disabled,
        });

        rows.push(row);
        stack.push(Frame {
            transform: desc_transform,
            clip: desc_clip,
            disabled,
            invisible,
            subtree_end: ends[i],
        });
    }
}

/// Hash everything that flows top-down into node `i`'s cascade row and
/// its arranged rect. If this matches prev *and* `subtree_hash[i]`
/// matches prev, every descendant's `(paint_rect, node_hash)` is
/// bit-identical to last frame by induction — damage can jump to
/// `subtree_end[i]` without diffing per node.
#[inline]
fn hash_cascade_input(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
    layout_rect: Rect,
    invisible: bool,
) -> CascadeInputHash {
    let mut h = Hasher::new();
    h.pod(&parent_transform);
    match parent_clip {
        Some(c) => {
            h.write_u8(1);
            h.pod(&c);
        }
        None => h.write_u8(0),
    }
    h.write_u8(u8::from(parent_dis));
    h.write_u8(u8::from(parent_inv));
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
) -> Rect {
    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };
    let mut paint_local = owner_local;
    if tree.records.shape_span()[node.index()].len > 0 {
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            if let TreeItem::ShapeRecord(_, s) = item {
                paint_local = paint_local.union(s.paint_bbox_local(layout_rect.size));
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
