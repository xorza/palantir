//! One frame's captured measure: the node columns, the per-subtree
//! descriptors that slice them, and the map that finds a descriptor by id.

use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::layout::cache;
use crate::layout::cache::node_arenas::NodeArenas;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::tree::Tree;
use crate::scene::tree::root_slot::RootSlot;
use crate::text::shaped_text::ShapedText;

/// One cacheable subtree's place in a [`MeasureSnapshot`]: what it was
/// measured as, and the spans of its rows in each arena.
#[derive(Clone, Copy, Debug)]
pub(super) struct ArenaSnapshot {
    pub(super) subtree_hash: ContentHash,
    pub(super) available: Size,
    pub(super) nodes: Span,
    pub(super) tracks: Span,
    pub(super) text_shapes: Span,
}

/// What a subtree is measured as: its root's id, its rolled authoring hash, and
/// the offer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubtreeKey {
    pub(crate) widget_id: WidgetId,
    pub(crate) subtree_hash: ContentHash,
    pub(crate) available: Size,
}

impl SubtreeKey {
    /// The key of the subtree at root `slot` of `tree`, offered what `slot` gives it on
    /// `surface`.
    pub(crate) fn of_root(tree: &Tree, slot: &RootSlot, surface: Rect) -> Self {
        let root = slot.first_node.idx();
        Self {
            widget_id: tree.records.widget_id()[root],
            subtree_hash: tree.rollups.layout_subtree[root],
            available: slot.available(surface),
        }
    }

    /// The same subtree under the same offer, bit for bit.
    pub(crate) fn matches(self, other: Self) -> bool {
        self.widget_id == other.widget_id
            && self.subtree_hash == other.subtree_hash
            && cache::same_offer(self.available, other.available)
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeasureSnapshot {
    pub(super) nodes: NodeArenas,
    pub(super) tracks: Vec<f32>,
    pub(super) text_shapes: Vec<ShapedText>,
    pub(super) snapshots: WidgetIdMap<u32>,
    pub(super) descs: Vec<ArenaSnapshot>,
    pub(super) desc_widget_ids: Vec<WidgetId>,
    /// The `desc_widget_ids` [`Self::snapshots`] was built for. Held beside the
    /// map so both travel through `end_frame`'s swap: the map is reusable exactly
    /// when a capture into this struct yields the same sequence. Compared whole,
    /// one `memcmp`. Refused: a 64-bit fold of the sequence, whose collision
    /// handed a capture another sequence's indices.
    snapshots_ids: Vec<WidgetId>,
}

impl MeasureSnapshot {
    /// Empty everything, retained descriptor map included.
    pub(super) fn forget_all(&mut self) {
        self.clear_capture();
        self.snapshots.clear();
        self.snapshots_ids.clear();
    }

    /// Empty the captured columns, keeping the descriptor map.
    pub(super) fn clear_capture(&mut self) {
        self.nodes.clear();
        self.tracks.clear();
        self.text_shapes.clear();
        self.descs.clear();
        self.desc_widget_ids.clear();
    }

    /// The descriptor captured for `widget_id`'s subtree, if any.
    #[inline]
    pub(super) fn desc(&self, widget_id: WidgetId) -> Option<&ArenaSnapshot> {
        let idx = *self.snapshots.get(&widget_id)? as usize;
        Some(&self.descs[idx])
    }

    /// Rebuild `snapshots` unless the capture produced the id sequence the
    /// retained map was built for. Returns whether it rebuilt.
    pub(super) fn refresh_snapshots(&mut self) -> bool {
        if self.snapshots_ids == self.desc_widget_ids {
            return false;
        }
        self.snapshots.clear();
        for (desc, widget_id) in self.desc_widget_ids.iter().copied().enumerate() {
            self.snapshots.insert(widget_id, desc as u32);
        }
        self.snapshots_ids.clone_from(&self.desc_widget_ids);
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::cache::measure_snapshot::MeasureSnapshot;
    use crate::primitives::identity::widget_id::WidgetId;

    /// The map is rebuilt exactly when the captured sequence differs from the one it was built
    /// for: the same ids again reuse it, the same ids in another order or one fewer rebuild it,
    /// and each id then maps to its own index. A forgotten snapshot over an empty capture keeps
    /// the empty map.
    #[test]
    fn the_map_follows_the_exact_id_sequence() {
        let [a, b, c] = ["a", "b", "c"].map(WidgetId::from_hash);
        let mut snapshot = MeasureSnapshot::default();
        let capture = |snapshot: &mut MeasureSnapshot, ids: &[WidgetId]| {
            snapshot.desc_widget_ids.clear();
            snapshot.desc_widget_ids.extend_from_slice(ids);
            snapshot.refresh_snapshots()
        };
        assert!(capture(&mut snapshot, &[a, b, c]));
        assert!(!capture(&mut snapshot, &[a, b, c]), "the same sequence");
        assert!(capture(&mut snapshot, &[b, a, c]), "reordered");
        assert_eq!(
            [a, b, c].map(|id| snapshot.snapshots.get(&id).copied()),
            [Some(1), Some(0), Some(2)]
        );
        assert!(capture(&mut snapshot, &[b, a]), "one fewer");
        assert_eq!(snapshot.snapshots.get(&c), None);
        snapshot.forget_all();
        assert!(!capture(&mut snapshot, &[]));
        assert!(snapshot.snapshots.is_empty());
    }
}
