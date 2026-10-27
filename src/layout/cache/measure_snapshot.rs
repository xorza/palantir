//! One frame's captured measure: the node columns, the per-subtree
//! descriptors that slice them, and the map that finds a descriptor by id.

use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::layout::cache;
use crate::layout::cache::node_arenas::NodeArenas;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
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
    /// Every root's key, in paint order.
    pub(super) roots: Vec<SubtreeKey>,
    /// Ordered fold over `desc_widget_ids`, rebuilt by every capture.
    pub(super) desc_identity: u64,
    /// The `desc_identity` [`Self::snapshots`] was built for. Held
    /// beside the map so both travel through `end_frame`'s swap: the map is
    /// reusable exactly when a capture into this struct yields the same
    /// identity.
    snapshots_identity: u64,
}

impl MeasureSnapshot {
    /// Empty everything, retained descriptor map included. Resetting
    /// `snapshots_identity` stops a later capture folding to the same value
    /// from reusing a map that is gone.
    pub(super) fn forget_all(&mut self) {
        self.clear_capture();
        self.snapshots.clear();
        self.snapshots_identity = 0;
    }

    /// Empty the captured columns, keeping the descriptor map.
    pub(super) fn clear_capture(&mut self) {
        self.nodes.clear();
        self.tracks.clear();
        self.text_shapes.clear();
        self.descs.clear();
        self.desc_widget_ids.clear();
        self.roots.clear();
        self.desc_identity = 0;
    }

    /// The descriptor captured for `widget_id`'s subtree, if any.
    #[inline]
    pub(super) fn desc(&self, widget_id: WidgetId) -> Option<&ArenaSnapshot> {
        let idx = *self.snapshots.get(&widget_id)? as usize;
        Some(&self.descs[idx])
    }

    /// Rebuild `snapshots` unless the capture produced the id sequence the
    /// retained map was built for. Returns whether it rebuilt.
    ///
    /// Equality is approximated by `desc_identity`. A collision is
    /// survivable (the `subtree_hash` check in `try_lookup` rejects it, costing
    /// a hit) but silent, so [`Self::snapshots_match_descs`] checks it in
    /// debug builds.
    pub(super) fn refresh_snapshots(&mut self) -> bool {
        if self.snapshots_identity == self.desc_identity {
            debug_assert!(
                self.snapshots_match_descs(),
                "desc_identity collided: the retained WidgetId → descriptor map \
                 disagrees with the descriptors just captured",
            );
            return false;
        }
        self.snapshots.clear();
        for (desc, widget_id) in self.desc_widget_ids.iter().copied().enumerate() {
            self.snapshots.insert(widget_id, desc as u32);
        }
        self.snapshots_identity = self.desc_identity;
        true
    }

    /// Every descriptor is reachable through `snapshots` at its own index.
    /// One probe each, so only for `debug_assert!`.
    fn snapshots_match_descs(&self) -> bool {
        self.snapshots.len() == self.desc_widget_ids.len()
            && self
                .desc_widget_ids
                .iter()
                .copied()
                .enumerate()
                .all(|(desc, widget_id)| {
                    self.snapshots.get(&widget_id).copied() == Some(desc as u32)
                })
    }
}
