use crate::primitives::{Rect, Sense, WidgetId};
use glam::Vec2;
use rustc_hash::FxHashMap;

/// One widget's hit-test entry from last frame: identity, screen-space rect
/// (clipped by ancestors), and effective `Sense` (with disabled/visibility
/// cascade applied).
#[derive(Clone, Copy, Debug)]
struct HitEntry {
    id: WidgetId,
    rect: Rect,
    sense: Sense,
}

/// Pre-order snapshot of the just-arranged tree, in the form needed for
/// hit-testing: each node's screen-space rect (clipped by ancestors), its
/// effective `Sense` (cascading disabled/visibility), and an ordered list to
/// reverse-iterate for topmost-first lookups.
///
/// Rebuilt every `Ui::end_frame` from the shared `Cascades` table. Owns no
/// cascade scratch of its own — that lives in `Cascades` so the encoder and
/// hit index can't drift.
#[derive(Default)]
pub(crate) struct HitIndex {
    entries: Vec<HitEntry>,
    /// `WidgetId → entries[idx]`. Populated alongside `entries` during
    /// `rebuild` so `rect_for` / `contains_id` are O(1) instead of O(n) —
    /// these run on every input event while an active widget is captured.
    /// Capacity is reused across frames; uniqueness of ids is enforced by
    /// `Ui::node`'s release assert.
    by_id: FxHashMap<WidgetId, u32>,
}

impl HitIndex {
    /// Reset the entry/by-id storage for a fresh rebuild. Capacity is
    /// retained across frames so the steady-state path is alloc-free.
    /// Pair with `push_entry` per node, called from `Cascades::rebuild`
    /// inside its pre-order walk.
    pub(crate) fn begin_rebuild(&mut self, capacity: usize) {
        self.entries.clear();
        self.by_id.clear();
        self.entries.reserve(capacity);
        self.by_id.reserve(capacity);
    }

    /// Append one node's hit entry. Caller (the cascade walk) has
    /// already applied disabled/invisible cascade to `sense` and
    /// intersected `rect` with the ancestor clip.
    #[inline]
    pub(crate) fn push_entry(&mut self, id: WidgetId, rect: Rect, sense: Sense) {
        self.by_id.insert(id, self.entries.len() as u32);
        self.entries.push(HitEntry { id, rect, sense });
    }

    /// Reverse-iter entries → topmost-first under pre-order paint walk.
    /// `filter` decides which `Sense` values participate (hoverable for hover,
    /// clickable for press/release).
    pub(crate) fn hit_test(&self, pos: Vec2, filter: impl Fn(Sense) -> bool) -> Option<WidgetId> {
        for e in self.entries.iter().rev() {
            if filter(e.sense) && e.rect.contains(pos) {
                return Some(e.id);
            }
        }
        None
    }

    pub(crate) fn rect_for(&self, id: WidgetId) -> Option<Rect> {
        self.by_id.get(&id).map(|&i| self.entries[i as usize].rect)
    }

    pub(crate) fn contains_id(&self, id: WidgetId) -> bool {
        self.by_id.contains_key(&id)
    }
}
