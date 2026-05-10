//! Per-frame post-arrange state.
//!
//! `Cascades` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Forest, &LayoutResult)` and produces
//! a fresh `CascadeResult` — per-tree per-node cascade rows plus a
//! global hit index, all populated in a single per-tree pre-order walk.
//! Downstream phases (damage diff, input hit-test, renderer encoder)
//! take `&CascadeResult` as their single frozen-state handle.

use crate::forest::Forest;
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::result::{LayerResult, LayoutResult};
use crate::primitives::{rect::Rect, transform::TranslateScale};
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::array;
use strum::EnumCount as _;

/// Resolved cascade row for one node: the transform/clip/invisible state
/// the node consumes for its own paint and hit-test, with ancestor state
/// already folded in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    pub(crate) transform: TranslateScale,
    pub(crate) clip: Option<Rect>,
    pub(crate) screen_rect: Rect,
    pub(crate) invisible: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HitEntry {
    pub(crate) id: WidgetId,
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

/// Read-only artifact of `Cascades::run`. Holds the per-tree cascade
/// rows (indexed by `NodeId.0` within the matching tree) and a global
/// `WidgetId`-keyed hit index.
pub(crate) struct CascadeResult {
    /// Per-layer per-node cascade rows. Same indexing as
    /// `Tree::records`: `rows[layer as usize][node.index()]`.
    pub(crate) rows: [Vec<Cascade>; Layer::COUNT],
    /// Pre-order rect/sense snapshot in the form hit-testing needs.
    /// Layers append in paint order so reverse iteration yields
    /// topmost-first.
    pub(crate) entries: Vec<HitEntry>,
    pub(crate) by_id: FxHashMap<WidgetId, u32>,
}

impl Default for CascadeResult {
    fn default() -> Self {
        Self {
            rows: array::from_fn(|_| Vec::new()),
            entries: Vec::new(),
            by_id: FxHashMap::default(),
        }
    }
}

impl CascadeResult {
    /// Reverse-iter entries → topmost-first under pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for
    /// hover, clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        for e in self.entries.iter().rev() {
            if filter(e.sense) && e.rect.contains(pos) {
                return Some(e.id);
            }
        }
        None
    }

    pub(crate) fn hit_test_focusable(&self, pos: Vec2) -> Option<WidgetId> {
        for e in self.entries.iter().rev() {
            if e.focusable && e.rect.contains(pos) {
                return Some(e.id);
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

#[derive(Default)]
pub(crate) struct Cascades {
    stack: Vec<Frame>,
    pub(crate) result: CascadeResult,
}

impl Cascades {
    /// Walk every tree in paint order; produce one `Cascade` row per
    /// node in each tree's slot, and append a global hit entry per
    /// node. Anchor offset for each layer is read from the layer's
    /// own `RootSlot.anchor_rect` — no parent transform plumbing is
    /// needed because trees never share NodeId space.
    pub(crate) fn run(&mut self, forest: &Forest, results: &LayoutResult) -> &CascadeResult {
        let r = &mut self.result;
        let total: usize = forest.trees.iter().map(|t| t.records.len()).sum();
        r.entries.clear();
        r.entries.reserve(total);
        r.by_id.clear();
        r.by_id.reserve(total);

        for (layer, tree) in forest.iter_paint_order() {
            let layout = &results[layer];
            let rows = &mut r.rows[layer as usize];
            rows.clear();
            rows.reserve(tree.records.len());
            self.stack.clear();
            run_tree(
                tree,
                layout,
                rows,
                &mut r.entries,
                &mut r.by_id,
                &mut self.stack,
            );
        }
        &self.result
    }
}

fn run_tree(
    tree: &Tree,
    layout: &LayerResult,
    rows: &mut Vec<Cascade>,
    entries: &mut Vec<HitEntry>,
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
        let invisible = parent_inv || !layout_col[i].visibility.is_visible();

        let screen_rect = parent_transform.apply_rect(layout.rect[id.index()]);
        let row = Cascade {
            transform: parent_transform,
            clip: parent_clip,
            screen_rect,
            invisible,
        };

        let node_transform = tree.bounds(id).transform;
        let desc_transform = match node_transform {
            Some(t) => row.transform.compose(t),
            None => row.transform,
        };
        let desc_clip = if attrs.clip_mode().is_clip() {
            Some(match row.clip {
                Some(c) => screen_rect.intersect(c),
                None => screen_rect,
            })
        } else {
            row.clip
        };

        let visible_rect = match parent_clip {
            Some(c) => screen_rect.intersect(c),
            None => screen_rect,
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
        entries.push(HitEntry {
            id: widget_id,
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
