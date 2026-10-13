//! One frame's captured measure: the node columns, the per-subtree
//! descriptors that slice them, and the map that finds a descriptor by id.

use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::layout::cache::AvailableKey;
use crate::layout::cache::node_arenas::NodeArenas;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::text::shaped_text::ShapedText;

/// One cacheable subtree's place in a [`MeasureSnapshot`]: what it was
/// measured as, and the spans of its rows in each arena.
#[derive(Clone, Copy, Debug)]
pub(super) struct ArenaSnapshot {
    pub(super) subtree_hash: ContentHash,
    pub(super) available_q: AvailableKey,
    pub(super) nodes: Span,
    pub(super) tracks: Span,
    pub(super) text_shapes: Span,
}

/// What a subtree is measured as: its root's id, its rolled authoring hash, and
/// the quantized offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubtreeKey {
    pub(crate) wid: WidgetId,
    pub(crate) subtree_hash: ContentHash,
    pub(crate) available_q: AvailableKey,
}

#[derive(Debug, Default)]
pub(crate) struct MeasureSnapshot {
    pub(super) nodes: NodeArenas,
    pub(super) tracks: Vec<f32>,
    pub(super) text_shapes: Vec<ShapedText>,
    pub(super) snapshots: WidgetIdMap<u32>,
    pub(super) descriptors: Vec<ArenaSnapshot>,
    pub(super) descriptor_wids: Vec<WidgetId>,
    /// Every root's key, in paint order.
    pub(super) roots: Vec<SubtreeKey>,
    /// Ordered fold over `descriptor_wids`, rebuilt by every capture.
    pub(super) descriptor_identity: u64,
    /// The `descriptor_identity` [`Self::snapshots`] was built for. Held
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
        self.descriptors.clear();
        self.descriptor_wids.clear();
        self.roots.clear();
        self.descriptor_identity = 0;
    }

    /// The descriptor captured for `wid`'s subtree, if any.
    #[inline]
    pub(super) fn descriptor(&self, wid: WidgetId) -> Option<&ArenaSnapshot> {
        let index = *self.snapshots.get(&wid)? as usize;
        Some(&self.descriptors[index])
    }

    /// Rebuild `snapshots` unless the capture produced the id sequence the
    /// retained map was built for. Returns whether it rebuilt.
    ///
    /// Equality is approximated by `descriptor_identity`. A collision is
    /// survivable (the `subtree_hash` check in `try_lookup` rejects it, costing
    /// a hit) but silent, so [`Self::snapshots_match_descriptors`] checks it in
    /// debug builds.
    pub(super) fn refresh_snapshots(&mut self) -> bool {
        if self.snapshots_identity == self.descriptor_identity {
            debug_assert!(
                self.snapshots_match_descriptors(),
                "descriptor_identity collided: the retained WidgetId → descriptor map \
                 disagrees with the descriptors just captured",
            );
            return false;
        }
        self.snapshots.clear();
        for (descriptor, wid) in self.descriptor_wids.iter().copied().enumerate() {
            self.snapshots.insert(wid, descriptor as u32);
        }
        self.snapshots_identity = self.descriptor_identity;
        true
    }

    /// Every descriptor is reachable through `snapshots` at its own index.
    /// One probe each, so only for `debug_assert!`.
    fn snapshots_match_descriptors(&self) -> bool {
        self.snapshots.len() == self.descriptor_wids.len()
            && self
                .descriptor_wids
                .iter()
                .copied()
                .enumerate()
                .all(|(descriptor, wid)| {
                    self.snapshots.get(&wid).copied() == Some(descriptor as u32)
                })
    }
}
