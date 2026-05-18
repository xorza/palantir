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
use crate::forest::shapes::record::{ShapeRecord, shadow_paint_rect_local};
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::input::sense::Sense;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{rect::Rect, transform::TranslateScale};
use glam::Vec2;
use rustc_hash::FxHashMap;
use soa_rs::{Soa, Soars};
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
    /// uses its own `EntryRow.rect`; shadows aren't clickable.
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
            shape_rects: array::from_fn(|_| Vec::new()),
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

    /// One reverse walk that finds the topmost match for both filters
    /// at once. Used on `PointerMoved` to recompute hover + scroll
    /// target without a second pass over `entries`.
    pub(crate) fn hit_test_pair(
        &self,
        pos: Vec2,
        a_filter: impl Fn(Sense) -> bool,
        b_filter: impl Fn(Sense) -> bool,
    ) -> HitPair {
        let rects = self.entries.rect();
        let senses = self.entries.sense();
        let ids = self.entries.widget_id();
        let mut a = None;
        let mut b = None;
        for i in (0..rects.len()).rev() {
            if !rects[i].contains(pos) {
                continue;
            }
            if a.is_none() && a_filter(senses[i]) {
                a = Some(ids[i]);
            }
            if b.is_none() && b_filter(senses[i]) {
                b = Some(ids[i]);
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
    // Two transforms apply to this node's paint:
    // - `parent_transform` for chrome (encoder emits chrome before the
    //   body push) and the chrome's drop shadow.
    // - `shape_transform = parent.compose(self)` for direct shapes
    //   (encoder emits shapes inside the body push, per the
    //   `Panel::transform` contract).
    // Chrome and shapes are unioned in screen space so each side
    // carries its own transform — folding them in owner-local would
    // lose `self.transform` for shapes.
    let self_transform = tree.transform_of(node).unwrap_or(TranslateScale::IDENTITY);
    let shape_transform = parent_transform.compose(self_transform);

    // Each accumulator is `Option<Rect>` rather than seeded to
    // `owner_local` / `Rect::ZERO`, because `Rect::union` is not
    // sound under either sentinel: zero-sized rects bias toward
    // the origin (see `Rect::union` doc), and seeding to the owner
    // rect would inflate paint-rect for non-painting containers and
    // for chromeless shape hosts — defeating tight damage on pan.
    let mut chrome_local: Option<Rect> = None;
    let mut shapes_local: Option<Rect> = None;
    if tree.records.shape_span()[node.idx()].len > 0 {
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            if let TreeItem::ShapeRecord(idx, s) = item {
                let bbox = s.paint_bbox_local(layout_rect.size);
                // `ShapeRecord::Text { local_origin: Some(_), .. }`
                // returns a zero-size bbox because the glyph extent
                // isn't known until cosmic-text shapes the run.
                // Conservatively fall back to the owner rect for the
                // paint-extent accumulator so the encoder's
                // subtree-paint-rect cull doesn't drop the text; the
                // per-shape `shape_rects[idx]` still stores the
                // origin-only rect (callers that key off the
                // per-shape rect, e.g. paint anims, get the precise
                // origin). Pinned by
                // `multi_shape_text_per_leaf_emits_one_drawtext_per_run_at_local_rect`.
                let extent_bbox = match s {
                    ShapeRecord::Text {
                        local_origin: Some(_),
                        ..
                    } => Rect {
                        min: Vec2::ZERO,
                        size: layout_rect.size,
                    },
                    _ => bbox,
                };
                shapes_local = Some(match shapes_local {
                    Some(acc) => acc.union(extent_bbox),
                    None => extent_bbox,
                });
                let tree_local = Rect {
                    min: layout_rect.min + bbox.min,
                    size: bbox.size,
                };
                let screen = clip_to(shape_transform.apply_rect(tree_local), parent_clip);
                shape_rects[idx as usize] = screen;
            }
        }
    }
    // Owner rect: contributed when this node paints chrome (so the
    // chrome rect and its shadow show up in damage / paint extent)
    // OR when it has a clip mode set (so the encoder's
    // subtree-paint-rect cull doesn't skip the node before its
    // PushClip/PopClip pair fires). Chromeless, clipless nodes
    // contribute only their direct shapes — that's the fix that
    // collapses pan-damage on a transformed shape host from
    // "full surface" to "swept shape band".
    let has_clip = tree.records.attrs()[node.idx()].clip_mode().is_clip();
    let chrome = tree.chrome(node);
    if chrome.is_some() || has_clip {
        let owner_local = Rect {
            min: Vec2::ZERO,
            size: layout_rect.size,
        };
        let mut local = owner_local;
        if let Some(bg) = chrome
            && !bg.shadow.is_noop()
        {
            let s = &bg.shadow;
            let g = s.geom();
            local = local.union(shadow_paint_rect_local(
                None,
                layout_rect.size,
                g.offset,
                g.blur,
                g.spread,
                s.inset(),
            ));
        }
        chrome_local = Some(local);
    }

    let chrome_screen = chrome_local.map(|local| {
        let tree_local = Rect {
            min: layout_rect.min + local.min,
            size: local.size,
        };
        clip_to(parent_transform.apply_rect(tree_local), parent_clip)
    });
    let shapes_screen = shapes_local.map(|local| {
        let tree_local = Rect {
            min: layout_rect.min + local.min,
            size: local.size,
        };
        clip_to(shape_transform.apply_rect(tree_local), parent_clip)
    });
    match (chrome_screen, shapes_screen) {
        (Some(c), Some(s)) => c.union(s),
        (Some(c), None) => c,
        (None, Some(s)) => s,
        (None, None) => Rect::ZERO,
    }
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
    /// land in `Cascades::shape_rects` at the *composed* transform
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

        // Shape is the first (and only) recorded shape; its rect lives
        // at `cascades.shape_rects[layer][0]`.
        let layer_idx = Layer::Main.idx();
        let shape_rect = ui.layout.cascades.shape_rects[layer_idx][0];
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
}
