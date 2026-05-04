//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Collision detection.** `record(id)` returns `false` and
//!    triggers an assert in `Ui::node` if the same `WidgetId` appears
//!    twice in one frame — duplicate ids silently corrupt every
//!    per-id store (focus, scroll, click capture, hit-test).
//! 2. **Removed-widget diff.** At `end_frame`, computes which ids
//!    were present last frame but absent this frame. Both
//!    [`crate::ui::damage::Damage`] and [`crate::text::TextMeasurer`]
//!    consume this list to evict per-widget state — sharing the diff
//!    keeps each consumer at `O(removed)` instead of `O(map)`.
//! 3. **Frame rollover.** `begin_frame` swaps `curr → prev` and
//!    clears `curr` — no clone, capacity retained both sides.

use crate::tree::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Default)]
pub(crate) struct SeenIds {
    /// `WidgetId`s recorded this frame so far. Populated by
    /// [`Self::record`] during `Ui::node`.
    curr: FxHashSet<WidgetId>,
    /// Last frame's `curr`. Diffed against this frame in
    /// [`Self::end_frame`].
    prev: FxHashSet<WidgetId>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated each frame; `Vec` because consumers only iterate.
    removed: Vec<WidgetId>,
    /// Per-original-id occurrence counter for auto-id collision
    /// disambiguation. Bumped by [`Self::next_dup`] when an auto id
    /// collides; cleared each frame.
    dup: FxHashMap<WidgetId, u32>,
}

impl SeenIds {
    /// Roll into a new frame: this-frame's `curr` becomes
    /// last-frame's `prev`, and the new `curr` is empty. Swap rather
    /// than clone — capacity stays on both sides.
    pub(crate) fn begin_frame(&mut self) {
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        self.dup.clear();
    }

    /// Record a `WidgetId` for this frame. Returns `false` if `id`
    /// was already recorded this frame — the caller (`Ui::node`)
    /// turns that into a panic with the call-site context.
    pub(crate) fn record(&mut self, id: WidgetId) -> bool {
        self.curr.insert(id)
    }

    /// Resolve an auto-generated id that has already collided this frame:
    /// derive a fresh id by mixing in the next occurrence counter for the
    /// original `id`, recording (and returning) the disambiguated id.
    /// Asserts the disambiguated id itself doesn't collide — if it does,
    /// the user has constructed an explicit `with_id` that shadows the
    /// auto-disambiguation slot, which is genuinely a bug.
    pub(crate) fn next_dup(&mut self, id: WidgetId) -> WidgetId {
        let counter = self.dup.entry(id).or_insert(0);
        *counter += 1;
        let disambiguated = id.with(*counter);
        assert!(
            self.curr.insert(disambiguated),
            "auto-id disambiguation collided with an explicit id ({disambiguated:?}) \
             — an explicit `.with_id(key)` produced the same hash as an auto-generated \
             id at occurrence {counter}. Pick a different explicit key.",
        );
        disambiguated
    }

    /// Compute the removed-widget list for this frame and return a
    /// borrow of it. Must be called once between recording and the
    /// consumers that fan the diff out (text cache eviction, damage
    /// rect accumulation, etc.).
    pub(crate) fn end_frame(&mut self) -> &[WidgetId] {
        self.removed.clear();
        for wid in &self.prev {
            if !self.curr.contains(wid) {
                self.removed.push(*wid);
            }
        }
        &self.removed
    }
}
