//! Per-node cascade resolution. Rebuilt every frame from `(&Tree,
//! &LayoutResult)`; consumed by both the renderer encoder (to skip invisible
//! subtrees) and the input hit index (to derive screen-space rects and
//! effective sense). Centralizing the cascade rules here means
//! disabled/invisible/clip/transform live in exactly one walk — encoder
//! and hit-index can no longer drift from each other.

use crate::layout::LayoutResult;
use crate::primitives::{Rect, TranslateScale};
use crate::tree::{NodeId, Tree};

/// Per-node ancestor-cascade state. `own_*` apply to this node's own paint
/// and hit-test; `descendant_*` are what this node passes down to children.
#[derive(Clone, Copy, Debug)]
pub struct NodeCascade {
    /// Cumulative transform that places this node's own rect into screen
    /// space. Equal to the parent's `descendant_transform`.
    pub own_transform: TranslateScale,
    /// Clip rect inherited from ancestors that this node's own paint /
    /// hit-test must be intersected with. Equal to the parent's
    /// `descendant_clip`. Stored in screen space so intersection composes
    /// directly with transformed rects.
    pub own_clip: Option<Rect>,
    /// Transform passed down to children. Composes `own_transform` with this
    /// node's own transform (if any).
    pub descendant_transform: TranslateScale,
    /// Clip rect passed down to children. If this node has `is_clip()`,
    /// the ancestor clip intersected with this node's own screen rect;
    /// otherwise just the inherited clip.
    pub descendant_clip: Option<Rect>,
    /// True if any ancestor (or self) has `is_disabled()`.
    pub effective_disabled: bool,
    /// True if any ancestor (or self) is non-`Visible`.
    pub effective_invisible: bool,
}

impl NodeCascade {
    const ROOT_PARENT: Self = Self {
        own_transform: TranslateScale::IDENTITY,
        own_clip: None,
        descendant_transform: TranslateScale::IDENTITY,
        descendant_clip: None,
        effective_disabled: false,
        effective_invisible: false,
    };
}

/// Flat per-node cascade table indexed by `NodeId.0`. Capacity is reused
/// across frames; alloc-free in steady state.
#[derive(Default)]
pub struct Cascades {
    nodes: Vec<NodeCascade>,
}

impl Cascades {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk `tree.nodes` in storage order (== pre-order, since recording is
    /// depth-first) and produce one `NodeCascade` per node, threading the
    /// four cascades through the parent's slot.
    pub fn rebuild(&mut self, tree: &Tree, layout: &LayoutResult) {
        self.nodes.clear();
        self.nodes.reserve(tree.node_count());

        // Walk pre-order. `stack` holds the cascade row + `subtree_end` for
        // each currently-open ancestor; the parent of node `i` is whichever
        // ancestor's subtree still contains `i` on top of the stack.
        let mut stack: Vec<(NodeCascade, u32)> = Vec::new();

        let paint = tree.paint_column();
        let layout_col = tree.layout_column();
        let subtree_end = tree.subtree_end_column();

        for i in 0..tree.node_count() {
            while let Some(&(_, end)) = stack.last() {
                if (i as u32) < end {
                    break;
                }
                stack.pop();
            }
            let parent = stack.last().map_or(NodeCascade::ROOT_PARENT, |&(c, _)| c);

            let id = NodeId(i as u32);
            let attrs = paint[i].attrs;

            let effective_disabled = parent.effective_disabled || attrs.is_disabled();
            let effective_invisible = parent.effective_invisible || !layout_col[i].is_visible();

            let own_transform = parent.descendant_transform;
            let own_clip = parent.descendant_clip;

            let node_transform = tree.read_extras(id).transform;
            let descendant_transform = match node_transform {
                Some(t) => own_transform.compose(t),
                None => own_transform,
            };

            let descendant_clip = if attrs.is_clip() {
                let screen_rect = own_transform.apply_rect(layout.rect(id));
                Some(match own_clip {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                own_clip
            };

            let row = NodeCascade {
                own_transform,
                own_clip,
                descendant_transform,
                descendant_clip,
                effective_disabled,
                effective_invisible,
            };
            self.nodes.push(row);
            stack.push((row, subtree_end[i]));
        }
    }

    pub fn at(&self, id: NodeId) -> NodeCascade {
        self.nodes[id.index()]
    }

    pub fn is_invisible(&self, id: NodeId) -> bool {
        self.nodes[id.index()].effective_invisible
    }
}
