//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Collision detection.** `record(id)` returns `false` and
//!    triggers an assert in `Ui::node` if the same `WidgetId` appears
//!    twice in one frame — duplicate ids silently corrupt every
//!    per-id store (focus, scroll, click capture, hit-test).
//! 2. **Removed-widget diff + rollover.** [`Self::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::ui::damage::Damage`] /
//!    [`crate::text::TextShaper`] / measure cache / state /
//!    animation), then swaps `curr → prev` so the next frame diffs
//!    against this one. Called once per `run_frame` from
//!    [`crate::Ui::paint_phase`]; discarded record passes don't touch
//!    seen-id state, so `prev` stays anchored at the last *painted*
//!    frame regardless of how many discard passes ran.

use crate::forest::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};

/// How a `WidgetId` was produced. Threaded into [`SeenIds::record`]
/// so collisions can be reported with the right diagnosis: `Auto`
/// collisions get silently disambiguated, `Explicit` collisions are
/// always caller bugs and hard-assert with a key-collision message.
#[derive(Clone, Copy, Debug)]
pub(crate) enum IdSource {
    /// Caller passed an explicit key via `.id_salt(...)`. Collisions
    /// are bugs.
    Explicit,
    /// Id minted by `WidgetId::auto_stable()` (track-caller). Collisions
    /// are expected in loops / helper closures and get disambiguated
    /// via an occurrence counter.
    Auto,
}

#[derive(Default)]
pub(crate) struct SeenIds {
    /// `WidgetId`s recorded this frame so far. Populated by
    /// [`Self::record`] during `Ui::node`.
    curr: FxHashSet<WidgetId>,
    /// Last *painted* frame's `curr`. Diffed against this pass's
    /// `curr` in [`Self::rollover`] and then replaced by it.
    prev: FxHashSet<WidgetId>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated by [`Self::rollover`]; consumers iterate via
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
    /// frame's recording, established by [`Self::rollover`]. A
    /// `run_frame` two-pass discard build calls `pre_record` then
    /// never reaches `rollover`, so `prev` must be preserved across
    /// the discard.
    pub(crate) fn pre_record(&mut self) {
        self.curr.clear();
        self.dup.clear();
    }

    /// Record a widget for this frame and return the id it should
    /// actually use. Auto ids that collide are silently disambiguated
    /// by mixing in an occurrence counter; explicit-id collisions are
    /// hard bugs and panic with call-site context.
    pub(crate) fn record(&mut self, id: WidgetId, source: IdSource) -> WidgetId {
        if self.curr.insert(id) {
            return id;
        }
        assert!(
            matches!(source, IdSource::Auto),
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

    /// Populate `self.removed` with widgets present in `prev` but
    /// absent from `curr`, then swap `curr → prev` so the next frame
    /// diffs against this one. Returns a borrow of `self.removed`
    /// for callers that want to fan the diff straight into per-widget
    /// caches (text shaper, measure cache, state map, animation,
    /// damage); the field stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &FxHashSet<WidgetId> {
        self.removed.clear();
        for wid in &self.prev {
            if !self.curr.contains(wid) {
                self.removed.insert(*wid);
            }
        }
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        &self.removed
    }
}
