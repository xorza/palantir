//! Per-node cascade resolution. Rebuilt every frame from `(&Tree,
//! &LayoutResult)`; consumed by both the renderer encoder (to skip invisible
//! subtrees) and the input hit index (to derive screen-space rects and
//! effective sense). Centralizing the cascade rules here means
//! disabled/invisible/clip/transform live in exactly one walk — encoder
//! and hit-index can no longer drift from each other.

use crate::layout::LayoutResult;
use crate::primitives::{Rect, TranslateScale};
use crate::tree::{NodeId, Tree};

/// Fields a node consumes for its own paint and hit-test. Always read
/// together by `HitIndex::rebuild`, so they live as one row.
#[derive(Clone, Copy, Debug)]
pub struct OwnCascade {
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

/// Fields a node passes down to its children. Read together by `rebuild`'s
/// open-ancestor stack and by any future encoder push/pop driver.
#[derive(Clone, Copy, Debug)]
pub struct DescendantCascade {
    pub transform: TranslateScale,
    pub clip: Option<Rect>,
}

/// Per-node cascade table indexed by `NodeId.0`, grouped by access
/// pattern: hit-test/paint reads `own[i]` as a row; rebuild + future
/// encoder push/pop reads `descendant[i]` as a row. Capacity reused
/// across frames; alloc-free in steady state.
#[derive(Default)]
pub struct Cascades {
    own: Vec<OwnCascade>,
    descendant: Vec<DescendantCascade>,
}

impl Cascades {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk `tree.nodes` in storage order (== pre-order, since recording is
    /// depth-first) and produce one `OwnCascade` + one `DescendantCascade`
    /// per node, threading the descendant row through an open-ancestor
    /// stack.
    pub fn rebuild(&mut self, tree: &Tree, layout: &LayoutResult) {
        let n = tree.node_count();
        self.own.clear();
        self.descendant.clear();
        self.own.reserve(n);
        self.descendant.reserve(n);

        struct Frame {
            descendant: DescendantCascade,
            disabled: bool,
            invisible: bool,
            subtree_end: u32,
        }
        const ROOT: (DescendantCascade, bool, bool) = (
            DescendantCascade {
                transform: TranslateScale::IDENTITY,
                clip: None,
            },
            false,
            false,
        );
        let mut stack: Vec<Frame> = Vec::new();

        let paint = tree.paint_column();
        let layout_col = tree.layout_column();
        let subtree_end = tree.subtree_end_column();

        for i in 0..n {
            while let Some(top) = stack.last() {
                if (i as u32) < top.subtree_end {
                    break;
                }
                stack.pop();
            }
            let (parent_desc, parent_dis, parent_inv) = match stack.last() {
                Some(p) => (p.descendant, p.disabled, p.invisible),
                None => ROOT,
            };

            let id = NodeId(i as u32);
            let attrs = paint[i].attrs;

            let disabled = parent_dis || attrs.is_disabled();
            let invisible = parent_inv || !layout_col[i].is_visible();

            let own = OwnCascade {
                transform: parent_desc.transform,
                clip: parent_desc.clip,
                disabled,
                invisible,
            };

            let node_transform = tree.read_extras(id).transform;
            let desc_transform = match node_transform {
                Some(t) => own.transform.compose(t),
                None => own.transform,
            };
            let desc_clip = if attrs.is_clip() {
                let screen_rect = own.transform.apply_rect(layout.rect(id));
                Some(match own.clip {
                    Some(c) => screen_rect.intersect(c),
                    None => screen_rect,
                })
            } else {
                own.clip
            };
            let descendant = DescendantCascade {
                transform: desc_transform,
                clip: desc_clip,
            };

            self.own.push(own);
            self.descendant.push(descendant);
            stack.push(Frame {
                descendant,
                disabled,
                invisible,
                subtree_end: subtree_end[i],
            });
        }
    }

    pub fn is_invisible(&self, id: NodeId) -> bool {
        self.own[id.index()].invisible
    }

    pub fn own_column(&self) -> &[OwnCascade] {
        &self.own
    }

    pub fn descendant_column(&self) -> &[DescendantCascade] {
        &self.descendant
    }
}
