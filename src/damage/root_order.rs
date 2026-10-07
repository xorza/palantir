//! The paint order of a layer's roots between frames.

use crate::cascade::layer_cascade::LayerCascade;
use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::row_matcher::ROW_UNMATCHED;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::per_layer::PerLayer;
use crate::scene::tree::Tree;

/// Each layer's roots from last frame in paint order: the child list of a virtual parent the layer would be as a node.
///
/// A node's children are paint rows, so swapped children flip the row order and the walk damages their overlap. A root has no parent row and no hash includes its position, so two overlapping roots raised past each other with the same content and rects would read as unchanged. Matched by id, so adding or removing a root shifts no other; only the order of roots both frames hold is compared.
#[derive(Debug, Default)]
pub(crate) struct RootOrder {
    prev: PerLayer<Vec<WidgetId>>,
    /// Scratch: last frame's roots of the layer being diffed with their positions, sorted by id.
    positions: Vec<(WidgetId, u32)>,
    /// Scratch: each current root's position last frame, or [`ROW_UNMATCHED`].
    matched: Vec<u32>,
    /// Scratch: each current root's painted extent.
    extents: Vec<Rect>,
    inversions: InvertedOverlaps,
}

impl RootOrder {
    /// Damage the overlap of every two roots of `layer` whose order flipped, into `out` unless `force_full` makes it moot, and remember this frame's order.
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
            self.inversions.push(out, &self.matched, &self.extents);
        }
        prev.clear();
        prev.extend(curr);
    }
}
