//! Per-node cascade resolution. Rebuilt every frame from `(&Tree,
//! &LayoutResult)`; consumed by both the renderer encoder (to skip invisible
//! subtrees) and the input hit index (to derive screen-space rects and
//! effective sense). Centralizing the cascade rules here means
//! disabled/invisible/clip/transform live in exactly one walk — encoder
//! and hit-index can no longer drift from each other.

use crate::layout::LayoutResult;
use crate::primitives::{Rect, TranslateScale};
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
    /// stack.
    pub fn rebuild(&mut self, tree: &Tree, layout: &LayoutResult) {
        let n = tree.node_count();
        self.rows.clear();
        self.rows.reserve(n);
        self.stack.clear();

        let paint = tree.paints();
        let layout_col = tree.layouts();
        let subtree_end = tree.subtree_ends();

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

            let row = Cascade {
                transform: parent_transform,
                clip: parent_clip,
                disabled,
                invisible,
            };

            let node_transform = tree.read_extras(id).transform;
            let desc_transform = match node_transform {
                Some(t) => row.transform.compose(t),
                None => row.transform,
            };
            let desc_clip = if attrs.is_clip() {
                let screen_rect = row.transform.apply_rect(layout.rect(id));
                Some(match row.clip {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                row.clip
            };

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
