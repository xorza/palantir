//! Per-frame post-arrange state.
//!
//! `Cascades` (the engine) owns the walk scratch + the result. Each
//! `run()` reads `(&Tree, &LayoutResult)` and produces a fresh
//! `CascadeResult` — the per-node cascade rows plus a hit index, both
//! populated in a single pre-order walk. Downstream phases (damage
//! diff, input hit-test, renderer encoder) take `&CascadeResult` as
//! their single frozen-state handle, so they read consistent values
//! without any "cascade vs hit-index" coordination.
//!
//! Mirrors `LayoutEngine` / `LayoutResult`: the engine carries internal
//! scratch capacities across frames; the result is the read-only
//! artifact downstream consumes.

use crate::layout::result::LayoutResult;
use crate::layout::types::sense::Sense;
use crate::primitives::{rect::Rect, transform::TranslateScale};
use crate::tree::widget_id::WidgetId;
use crate::tree::{NodeId, Tree};
use glam::Vec2;
use rustc_hash::FxHashMap;

/// Resolved cascade row for one node: the transform/clip/invisible state
/// the node consumes for its own paint and hit-test, with ancestor state
/// already folded in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    /// Cumulative transform that places this node's own rect into screen
    /// space.
    pub(crate) transform: TranslateScale,
    /// Ancestor clip (screen space) the node's own rect/sense must be
    /// intersected with.
    pub(crate) clip: Option<Rect>,
    /// Node's layout rect projected into screen space via `transform`.
    /// Cached here so encoder, hit-index, damage diff, and prev_frame
    /// snapshot all read the same value without re-running the math.
    pub(crate) screen_rect: Rect,
    /// True if any ancestor (or self) is non-`Visible`.
    pub(crate) invisible: bool,
}

/// One widget's hit-test entry: identity, screen-space rect (clipped by
/// ancestors), effective `Sense`, and focus eligibility — all with the
/// disabled/visibility cascade applied. Stored on `CascadeResult`'s
/// internal hit index, in pre-order so reverse iteration yields
/// topmost-first lookups.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitEntry {
    pub(crate) id: WidgetId,
    pub(crate) rect: Rect,
    pub(crate) sense: Sense,
    pub(crate) focusable: bool,
}

/// Open-ancestor frame on the rebuild walk's stack. Carries the resolved
/// state to fold into descendant nodes plus the pre-order span end so we
/// know when to pop. Module-private; lives on `Cascades` so the stack's
/// capacity is retained across frames.
struct Frame {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    subtree_end: u32,
}

/// Read-only artifact of `Cascades::run`. Holds the per-node cascade
/// rows and a `WidgetId`-keyed hit index. Downstream phases (damage,
/// input, renderer) take `&CascadeResult` and never mutate it.
#[derive(Default)]
pub(crate) struct CascadeResult {
    /// Per-node cascade rows in storage order (== pre-order).
    pub(crate) rows: Vec<Cascade>,
    /// Pre-order rect/sense snapshot in the form hit-testing needs.
    /// Indexed by pre-order position; `by_id` resolves a `WidgetId` in O(1).
    pub(crate) entries: Vec<HitEntry>,
    /// `WidgetId → entries[idx]`. Capacity reused across frames;
    /// uniqueness is enforced upstream by `Ui::node`'s collision assert.
    pub(crate) by_id: FxHashMap<WidgetId, u32>,
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

    /// Topmost focusable widget under `pos`, or `None`. Independent of
    /// `Sense` — a widget can be focusable without being clickable, and
    /// vice versa. Disabled / invisible nodes are excluded by the same
    /// cascade that nulls their `Sense`.
    pub(crate) fn hit_test_focusable(&self, pos: Vec2) -> Option<WidgetId> {
        for e in self.entries.iter().rev() {
            if e.focusable && e.rect.contains(pos) {
                return Some(e.id);
            }
        }
        None
    }
}

/// Per-frame engine that produces a `CascadeResult` from `(&Tree,
/// &LayoutResult)`. Holds the pre-order ancestor stack as scratch;
/// capacities (stack, rows, entries, by_id) are retained across frames
/// so steady-state runs are alloc-free.
#[derive(Default)]
pub(crate) struct Cascades {
    stack: Vec<Frame>,
    pub(crate) result: CascadeResult,
}

impl Cascades {
    /// Walk `tree.nodes` in storage order (== pre-order) and produce
    /// one `Cascade` row + one hit entry per node. Threads the
    /// descendant transform/clip/disabled/invisible through an
    /// open-ancestor stack; hit-entry derivation (clip-intersected rect
    /// + sense-cascaded effective sense) rides along the same loop.
    pub(crate) fn run(&mut self, tree: &Tree, layout: &LayoutResult) -> &CascadeResult {
        let n = tree.layout.len();
        let r = &mut self.result;
        r.rows.clear();
        r.rows.reserve(n);
        r.entries.clear();
        r.entries.reserve(n);
        r.by_id.clear();
        r.by_id.reserve(n);
        self.stack.clear();

        let paint = &tree.paint;
        let layout_col = &tree.layout;
        let subtree_end = &tree.subtree_end;
        let widget_ids = &tree.widget_ids;

        for i in 0..n {
            while let Some(top) = self.stack.last() {
                if (i as u32) < top.subtree_end {
                    break;
                }
                self.stack.pop();
            }
            let (parent_transform, parent_clip, parent_dis, parent_inv) = match self.stack.last() {
                Some(p) => (p.transform, p.clip, p.disabled, p.invisible),
                None => (TranslateScale::IDENTITY, None, false, false),
            };

            let id = NodeId(i as u32);
            let attrs = paint[i].attrs;

            let disabled = parent_dis || attrs.is_disabled();
            let invisible = parent_inv || !layout_col[i].visibility.is_visible();

            let screen_rect = parent_transform.apply_rect(layout.rect[id.index()]);
            let row = Cascade {
                transform: parent_transform,
                clip: parent_clip,
                screen_rect,
                invisible,
            };

            let node_transform = tree.read_extras(id).transform;
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

            // Hit entry: same per-node data, just clipped + sense-cascaded.
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
            r.by_id.insert(widget_id, r.entries.len() as u32);
            r.entries.push(HitEntry {
                id: widget_id,
                rect: visible_rect,
                sense,
                focusable,
            });

            r.rows.push(row);
            self.stack.push(Frame {
                transform: desc_transform,
                clip: desc_clip,
                disabled,
                invisible,
                subtree_end: subtree_end[i],
            });
        }
        &self.result
    }
}
