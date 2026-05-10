//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Collision detection.** `record(id)` returns `false` and
//!    triggers an assert in `Ui::node` if the same `WidgetId` appears
//!    twice in one frame — duplicate ids silently corrupt every
//!    per-id store (focus, scroll, click capture, hit-test).
//! 2. **Removed-widget diff.** [`Self::diff_for_sweep`] computes
//!    which ids were present last painted frame but absent this
//!    pass. Both [`crate::ui::damage::Damage`] and
//!    [`crate::text::TextShaper`] consume this list to evict
//!    per-widget state — sharing the diff keeps each consumer at
//!    `O(removed)` instead of `O(map)`.
//! 3. **Frame rollover.** The `curr → prev` swap is split out into
//!    [`Self::commit_rollover`], called from `Ui`'s paint phase only.
//!    Discarded `run_frame` passes (input-action drain or relayout
//!    discard) call `diff_for_sweep` but skip `commit_rollover`, so
//!    `prev` stays anchored at the last *painted* frame regardless of
//!    how many discard passes ran.

use crate::forest::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Default)]
pub(crate) struct SeenIds {
    /// `WidgetId`s recorded this frame so far. Populated by
    /// [`Self::record`] during `Ui::node`.
    curr: FxHashSet<WidgetId>,
    /// Last *painted* frame's `curr`. Diffed against this pass's
    /// `curr` in [`Self::diff_for_sweep`]; rolled forward by
    /// [`Self::commit_rollover`] only when this pass actually paints.
    prev: FxHashSet<WidgetId>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated by [`Self::diff_for_sweep`]; consumers iterate via
    /// a shared borrow on the field. Public-in-crate so callers can
    /// hold `&seen.removed` across other shared `&forest` reads — an
    /// accessor returning `&[..]` would tie the returned slice to the
    /// `&mut self` and block those reads. Stored as a `FxHashSet`
    /// (not `Vec`) so consumers that test per-row membership
    /// (`anim`, `text`) get O(1) lookups without rebuilding the set.
    pub(crate) removed: FxHashSet<WidgetId>,
    /// Per-original-id occurrence counter for auto-id collision
    /// disambiguation. Bumped by [`Self::next_dup`] when an auto id
    /// collides; cleared each frame.
    dup: FxHashMap<WidgetId, u32>,
}

impl SeenIds {
    /// Reset per-build state at the top of a frame. Clears the
    /// `curr` recording set + the auto-id disambiguation counter.
    /// **Doesn't touch `prev`** — that holds the last *painted*
    /// frame's recording, established by [`Self::commit_rollover`].
    /// A run_frame two-pass discard build calls `begin_frame` then
    /// never reaches `commit_rollover`, so `prev` must be preserved
    /// across the discard.
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
    /// in `curr`. **Doesn't** commit the rollover — `prev` keeps its
    /// last-painted-frame snapshot. Callers read `&seen.removed` to
    /// fan the diff out to consumers (text cache eviction, damage
    /// rect accumulation, etc.). Safe to call multiple times across
    /// run_frame passes; each call recomputes `removed` against the
    /// same `prev`.
    pub(crate) fn diff_for_sweep(&mut self) {
        self.removed.clear();
        for wid in &self.prev {
            if !self.curr.contains(wid) {
                self.removed.insert(*wid);
            }
        }
    }

    /// Commit the rollover: this pass becomes the new `prev` snapshot.
    /// Called from the painted pass only. A discarded record-only pass
    /// (run_frame two-pass mode) calls `diff_for_sweep` but skips
    /// `commit_rollover`, so the painted pass still diffs against the
    /// true last-painted frame.
    pub(crate) fn commit_rollover(&mut self) {
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
    }
}
