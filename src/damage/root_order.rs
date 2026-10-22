//! The paint order of a layer's roots between frames.

use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::layer_frame::LayerFrame;
use crate::damage::row_matcher::ROW_UNMATCHED;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::per_layer::PerLayer;
use crate::scene::tree::Tree;

/// Each layer's roots from last frame in paint order: the child list of a virtual parent the layer would be as a node.
///
/// A node's children are paint rows, so swapped children flip the row order and the walk damages their overlap. A root has no parent row and no hash includes its position, so two overlapping roots raised past each other with the same content and rects would read as unchanged. Matched by id, so adding or removing a root shifts no other; only the order of roots both frames hold is compared.
#[derive(Debug, Default)]
pub(super) struct RootOrder {
    prev: PerLayer<Vec<WidgetId>>,
    /// Scratch: last frame's roots of the layer being diffed with their positions, sorted by id.
    poss: Vec<(WidgetId, u32)>,
    /// Scratch: each current root's position last frame, or [`ROW_UNMATCHED`].
    matched: Vec<u32>,
    /// Scratch: each current root's painted extent.
    extents: Vec<Rect>,
    inversions: InvertedOverlaps,
}

impl RootOrder {
    /// Damage into `out` the overlap of every two roots of `frame`'s layer whose order flipped, and remember this frame's order.
    pub(super) fn diff(&mut self, frame: LayerFrame<'_>, out: &mut Vec<Rect>) {
        let LayerFrame {
            layer,
            tree,
            cascade,
        } = frame;
        let ids = tree.records.widget_id();
        let prev = &self.prev[layer];
        if prev.iter().copied().eq(roots(tree)) {
            return;
        }
        if !prev.is_empty() {
            self.poss.clear();
            self.poss
                .extend(prev.iter().enumerate().map(|(at, id)| (*id, at as u32)));
            self.poss.sort_unstable_by_key(|(id, _)| id.0);
            self.matched.clear();
            self.extents.clear();
            for slot in &tree.roots {
                let id = ids[slot.first_node.idx()];
                let at = self
                    .poss
                    .binary_search_by_key(&id.0, |(id, _)| id.0)
                    .map_or(ROW_UNMATCHED, |found| self.poss[found].1);
                self.matched.push(at);
                self.extents
                    .push(cascade.subtree_paint_rects[slot.first_node.idx()]);
            }
            self.inversions.push(out, &self.matched, &self.extents);
        }
        self.remember(frame);
    }

    /// Remember `frame`'s root order and damage nothing, for a frame that repaints every root anyway.
    pub(super) fn remember(&mut self, frame: LayerFrame<'_>) {
        let prev = &mut self.prev[frame.layer];
        prev.clear();
        prev.extend(roots(frame.tree));
    }
}

/// `tree`'s roots in paint order.
fn roots(tree: &Tree) -> impl Iterator<Item = WidgetId> + '_ {
    let ids = tree.records.widget_id();
    tree.roots
        .iter()
        .map(move |slot| ids[slot.first_node.idx()])
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::root_order::RootOrder;
    use crate::internals::scratch_report;

    impl RootOrder {
        /// Leaves out `prev`, the order the next frame diffs against.
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                prev: _,
                poss,
                matched,
                extents,
                inversions,
            } = self;
            scratch_report::vec_bytes(poss)
                + scratch_report::vec_bytes(matched)
                + scratch_report::vec_bytes(extents)
                + inversions.scratch_bytes()
        }
    }
}
