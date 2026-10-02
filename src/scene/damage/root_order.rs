//! The paint order of a layer's roots between frames.

use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::LayerCascade;
use crate::scene::damage;
use crate::scene::damage::row_matcher::ROW_UNMATCHED;
use crate::scene::layer::{Layer, PerLayer};
use crate::scene::tree::Tree;

/// Each layer's roots, last frame, in paint order — the child list of a
/// virtual parent the layer would be if it were a node.
///
/// A node's children are paint rows of the node, so two children that
/// swap order flip the node's row order, and the walk damages their
/// overlap. A root has no parent to hold that row, and no hash a root
/// carries includes its position. Two overlapping roots raised past each
/// other with the same content and rects would otherwise read as
/// unchanged, and the overlap would keep the old stacking.
///
/// Matched by id, so a root added or removed shifts no other root: the
/// order of the roots both frames hold is what is compared.
#[derive(Debug, Default)]
pub(crate) struct RootOrder {
    prev: PerLayer<Vec<WidgetId>>,
    /// Scratch: last frame's roots of the layer being diffed, with each
    /// one's position, sorted by id.
    positions: Vec<(WidgetId, u32)>,
    /// Scratch: for each current root, its position last frame, or
    /// [`ROW_UNMATCHED`].
    matched: Vec<u32>,
    /// Scratch: each current root's painted extent.
    extents: Vec<Rect>,
}

impl RootOrder {
    /// Damage the overlap of every two roots of `layer` whose order
    /// flipped since last frame, into `out` unless `force_full` makes the
    /// region moot, and remember this frame's order.
    pub(crate) fn diff(
        &mut self,
        layer: Layer,
        tree: &Tree,
        cascade: &LayerCascade,
        force_full: bool,
        out: &mut Vec<Rect>,
    ) {
        let ids = tree.records.widget_id();
        let prev = &mut self.prev[layer];
        let curr = tree.roots.iter().map(|slot| ids[slot.first_node.idx()]);
        if prev.iter().copied().eq(curr.clone()) {
            return;
        }
        if !force_full && !prev.is_empty() {
            self.positions.clear();
            self.positions
                .extend(prev.iter().enumerate().map(|(at, id)| (*id, at as u32)));
            self.positions.sort_unstable_by_key(|(id, _)| id.0);
            self.matched.clear();
            self.extents.clear();
            for slot in &tree.roots {
                let id = ids[slot.first_node.idx()];
                let at = self
                    .positions
                    .binary_search_by_key(&id.0, |(id, _)| id.0)
                    .map_or(ROW_UNMATCHED, |found| self.positions[found].1);
                self.matched.push(at);
                self.extents
                    .push(cascade.subtree_paint_rects[slot.first_node.idx()]);
            }
            damage::push_inverted_overlaps(out, &self.matched, &self.extents);
        }
        prev.clear();
        prev.extend(curr);
    }
}
