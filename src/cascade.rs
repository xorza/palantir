//! Per-node cascade resolution. Rebuilt every frame from `(&Tree,
//! &LayoutResult)`; consumed by the renderer encoder (to skip invisible
//! subtrees), damage diff (to read screen-space rects), and the
//! `HitIndex` (populated in this same walk so cascade resolution and
//! hit-entry flattening share one O(n) pass).
//!
//! Centralizing the cascade rules here means
//! disabled/invisible/clip/transform live in exactly one walk — encoder,
//! damage, and hit-index can no longer drift from each other.

use crate::input::HitIndex;
use crate::layout::LayoutResult;
use crate::primitives::{Rect, Sense, TranslateScale};
use crate::tree::{NodeId, Tree};

/// Resolved cascade row for one node: the transform/clip/disabled/invisible
/// state the node consumes for its own paint and hit-test, with ancestor
/// state already folded in. Always read together by `HitIndex::rebuild`.
#[derive(Clone, Copy, Debug)]
pub struct Cascade {
    /// Cumulative transform that places this node's own rect into screen
    /// space.
    pub transform: TranslateScale,
    /// Ancestor clip (screen space) the node's own rect/sense must be
    /// intersected with.
    pub clip: Option<Rect>,
    /// Node's layout rect projected into screen space via `transform`.
    /// Cached here so encoder, hit-index, damage diff, and prev_frame
    /// snapshot all read the same value without re-running the math.
    pub screen_rect: Rect,
    /// True if any ancestor (or self) is disabled.
    pub disabled: bool,
    /// True if any ancestor (or self) is non-`Visible`.
    pub invisible: bool,
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

/// Per-node cascade table indexed by `NodeId.0`. Capacity reused across
/// frames; alloc-free in steady state.
#[derive(Default)]
pub struct Cascades {
    rows: Vec<Cascade>,
    /// Per-rebuild ancestor stack. Cleared at the top of every `rebuild`,
    /// capacity retained — keeps the walk alloc-free in steady state.
    stack: Vec<Frame>,
}

impl Cascades {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk `tree.nodes` in storage order (== pre-order, since recording is
    /// depth-first) and produce one `Cascade` row per node, threading the
    /// descendant transform/clip/disabled/invisible through an open-ancestor
    /// stack. Same loop also writes one `HitIndex` entry per node — the
    /// hit-test view is a flat function of the cascade row + widget_id +
    /// effective sense, so producing it inline saves a second O(n) pass.
    pub(crate) fn rebuild(&mut self, tree: &Tree, layout: &LayoutResult, hit_index: &mut HitIndex) {
        let n = tree.node_count();
        self.rows.clear();
        self.rows.reserve(n);
        self.stack.clear();
        hit_index.begin_rebuild(n);

        let paint = tree.paints();
        let layout_col = tree.layouts();
        let subtree_end = tree.subtree_ends();
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
            let invisible = parent_inv || !layout_col[i].is_visible();

            let screen_rect = parent_transform.apply_rect(layout.rect(id));
            let row = Cascade {
                transform: parent_transform,
                clip: parent_clip,
                screen_rect,
                disabled,
                invisible,
            };

            let node_transform = tree.read_extras(id).transform;
            let desc_transform = match node_transform {
                Some(t) => row.transform.compose(t),
                None => row.transform,
            };
            let desc_clip = if attrs.is_clip() {
                Some(match row.clip {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                row.clip
            };

            // Hit-entry: same per-node data, just clipped + sense-cascaded.
            let visible_rect = match parent_clip {
                Some(c) => screen_rect.intersect(c),
                None => screen_rect,
            };
            let sense = if disabled || invisible {
                Sense::NONE
            } else {
                attrs.sense()
            };
            hit_index.push_entry(widget_ids[i], visible_rect, sense);

            self.rows.push(row);
            self.stack.push(Frame {
                transform: desc_transform,
                clip: desc_clip,
                disabled,
                invisible,
                subtree_end: subtree_end[i],
            });
        }
    }

    pub fn is_invisible(&self, id: NodeId) -> bool {
        self.rows[id.index()].invisible
    }

    pub fn is_disabled(&self, id: NodeId) -> bool {
        self.rows[id.index()].disabled
    }

    pub fn rows(&self) -> &[Cascade] {
        &self.rows
    }
}
