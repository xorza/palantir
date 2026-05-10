//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Collision detection.** `record(id)` returns `false` and
//!    triggers an assert in `Ui::node` if the same `WidgetId` appears
//!    twice in one frame — duplicate ids silently corrupt every
//!    per-id store (focus, scroll, click capture, hit-test).
//! 2. **Removed-widget diff.** At `end_frame`, computes which ids
//!    were present last frame but absent this frame. Both
//!    [`crate::ui::damage::Damage`] and [`crate::text::TextShaper`]
//!    consume this list to evict per-widget state — sharing the diff
//!    keeps each consumer at `O(removed)` instead of `O(map)`.
//! 3. **Frame rollover.** The `curr → prev` swap happens in
//!    `end_frame` (after the diff), NOT `begin_frame`. This is
//!    load-bearing for `Ui::run_frame`'s two-pass mode: the discard
//!    pass calls `begin_frame` + `record` but never `end_frame`, so
//!    its recording must not overwrite the last painted frame's
//!    snapshot. Putting the swap on the commit point (end_frame)
//!    keeps `prev` pointed at the *last painted* frame regardless of
//!    how many discard passes ran.

use crate::forest::widget_id::WidgetId;
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
    /// Repopulated by [`Self::end_frame`]; consumers iterate via a
    /// shared borrow on the field. Public-in-crate so callers can
    /// hold `&seen.removed` across other shared `&forest` reads — a
    /// `fn end_frame(&mut self) -> &[..]` accessor would tie the
    /// returned slice to the `&mut self` and block those reads.
    pub(crate) removed: Vec<WidgetId>,
    /// Per-original-id occurrence counter for auto-id collision
    /// disambiguation. Bumped by [`Self::next_dup`] when an auto id
    /// collides; cleared each frame.
    dup: FxHashMap<WidgetId, u32>,
}

impl SeenIds {
    /// Reset per-build state at the top of a frame. Clears the
    /// `curr` recording set + the auto-id disambiguation counter.
    /// **Doesn't touch `prev`** — that holds the last *painted* frame's
    /// recording, established by [`Self::end_frame`]. A run_frame
    /// two-pass discard build calls `begin_frame` then never reaches
    /// `end_frame`, so `prev` must be preserved across the discard.
    pub(crate) fn begin_frame(&mut self) {
        self.curr.clear();
        self.dup.clear();
    }

    /// Record a widget for this frame and return the id it should
    /// actually use. Auto ids that collide are silently disambiguated
    /// by mixing in an occurrence counter; explicit-id collisions are
    /// hard bugs and panic with call-site context.
    pub(crate) fn record(&mut self, id: WidgetId, auto: bool) -> WidgetId {
        if self.curr.insert(id) {
            return id;
        }
        assert!(
            auto,
            "WidgetId collision — id {id:?} recorded twice this frame. \
             Two explicit `.id_salt(key)` calls produced the same hash; \
             pick distinct keys. Duplicate ids silently corrupt focus, \
             scroll, click capture, and hit-testing.",
        );
        let counter = self.dup.entry(id).or_insert(0);
        *counter += 1;
        let disambiguated = id.with(*counter);
        assert!(
            self.curr.insert(disambiguated),
            "auto-id disambiguation collided with an explicit id ({disambiguated:?}) \
             — an explicit `.id_salt(key)` produced the same hash as an auto-generated \
             id at occurrence {counter}. Pick a different explicit key.",
        );
        disambiguated
    }

    /// Populate `self.removed` with widgets present in `prev` but not
    /// in `curr`, then commit the rollover (`curr → prev`). Callers
    /// then read `&seen.removed` to fan the diff out to consumers
    /// (text cache eviction, damage rect accumulation, etc.). The
    /// swap is the "this frame is committed" signal — deliberately
    /// HERE rather than in `begin_frame` so a discarded recording
    /// (run_frame two-pass mode) doesn't overwrite the last painted
    /// frame's snapshot.
    pub(crate) fn end_frame(&mut self) {
        self.removed.clear();
        for wid in &self.prev {
            if !self.curr.contains(wid) {
                self.removed.push(*wid);
            }
        }
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
    }
}
