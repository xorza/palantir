//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Eager disambiguation.** [`Self::resolve`] runs at
//!    `Ui::make_persistent_id` time — *before* the matching `ui.node`
//!    opens the actual record. It rewrites the resolved id by mixing
//!    in an occurrence counter when the raw id has already been
//!    handed out this frame, so the returned id matches what the
//!    tree, cascade, and `response_for` will see. Per-id state
//!    (focus, scroll, capture, hit-test) stays positional within the
//!    colliding call site. Explicit-key collisions (`.id(X)`,
//!    `.id_salt(X)`) are caller bugs: `resolve` queues a
//!    [`PendingExplicitCollision`] for the second occurrence and
//!    [`Self::record_endpoint`] finalizes the [`CollisionRecord`]
//!    once both opens have provided their `Endpoint`s.
//! 2. **Endpoint tracking.** [`Self::record_endpoint`] runs at
//!    `Forest::open_node` time, after the final id has been threaded
//!    through `Ui::node`'s parameter. Stores `final_id → Endpoint` so
//!    the magenta debug overlay has both halves of a collision pair
//!    on hand.
//! 3. **Removed-widget diff + rollover.** [`Self::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::ui::damage::DamageEngine`] /
//!    [`crate::text::TextShaper`] / measure cache / state /
//!    animation), then swaps `curr → prev` so the next frame diffs
//!    against this one. Called once per `run_frame` from
//!    [`crate::Ui::finalize_frame`]; discarded record passes don't
//!    touch seen-id state, so `prev` stays anchored at the last
//!    *painted* frame regardless of how many discard passes ran.

use crate::forest::Layer;
use crate::forest::tree::NodeId;
use crate::primitives::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};

/// One collision endpoint — a node together with its originating
/// layer. Both halves of a `CollisionRecord` are `Endpoint`s so the
/// encoder can resolve each side's arranged rect without a tree
/// scan, even when the two endpoints straddle a `push_layer`
/// boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) layer: Layer,
    pub(crate) node: NodeId,
}

/// One side of a queued explicit-collision pair. The first endpoint
/// is looked up by `first_raw_id` (the un-disambiguated id of the
/// first occurrence, already recorded in `curr` when this entry is
/// queued); the second endpoint is filled in at
/// [`SeenIds::record_endpoint`] when `second_final_id` is opened.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingExplicitCollision {
    pub(crate) first_raw_id: WidgetId,
    pub(crate) second_final_id: WidgetId,
}

#[derive(Default)]
pub(crate) struct SeenIds {
    /// Per-raw-id occurrence counter. Bumped inside [`Self::resolve`]
    /// every time a raw id is handed out — first call returns
    /// `raw_id`, second returns `raw_id.with(1)`, third `.with(2)`,
    /// etc. Cleared each frame in [`Self::pre_record`]. Independent
    /// of [`Self::curr`] so disambiguation doesn't depend on the
    /// `(layer, node)` of the actual record — `make_persistent_id`
    /// gives the right id without peeking at `Tree::peek_next_id`.
    counters: FxHashMap<WidgetId, u32>,
    /// `final_id → Endpoint` of every widget actually opened this
    /// frame. Populated by [`Self::record_endpoint`] from
    /// `Forest::open_node`. Read for explicit-collision endpoint
    /// resolution (the first endpoint lives under `raw_id`, which is
    /// the un-disambiguated form of any subsequent occurrence). Same
    /// keys feed the [`Self::rollover`] removed-diff.
    curr: FxHashMap<WidgetId, Endpoint>,
    /// Last *painted* frame's `curr`. Only the keys matter for the
    /// rollover diff — values are stale across frames. Same type as
    /// `curr` so `std::mem::swap` is alloc-free.
    prev: FxHashMap<WidgetId, Endpoint>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated by [`Self::rollover`]; consumers iterate via a
    /// shared borrow on the field. Public-in-crate so callers can
    /// hold `&seen.removed` across other shared `&forest` reads — an
    /// accessor returning `&[..]` would tie the returned slice to the
    /// `&mut self` and block those reads.
    pub(crate) removed: FxHashSet<WidgetId>,
    /// Explicit collisions queued by [`Self::resolve`] awaiting
    /// endpoint resolution at [`Self::record_endpoint`]. Each entry
    /// names the first occurrence's raw id (whose endpoint is already
    /// in `curr`) and the second occurrence's final id (whose
    /// endpoint arrives when `record_endpoint` opens it). Cleared
    /// each frame.
    pending: Vec<PendingExplicitCollision>,
}

impl SeenIds {
    /// Reset per-frame state at the top of a frame. Clears the
    /// `curr` recording map + the disambiguation counter + pending
    /// collisions. **Doesn't touch `prev`** — that holds the last
    /// *painted* frame's recording, established by [`Self::rollover`].
    /// A `run_frame` two-pass discard build calls `pre_record` then
    /// never reaches `rollover`, so `prev` must be preserved across
    /// the discard.
    pub(crate) fn pre_record(&mut self) {
        self.counters.clear();
        self.curr.clear();
        self.pending.clear();
    }

    /// Eagerly resolve a raw id to its disambiguated final id. Bumps
    /// the per-raw-id counter so the next call with the same raw id
    /// returns the next occurrence slot. If the salt was explicit
    /// and this is a collision (counter was already > 0), queues a
    /// [`PendingExplicitCollision`] so [`Self::record_endpoint`] can
    /// emit a [`crate::forest::CollisionRecord`] once both endpoints
    /// are known.
    pub(crate) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> WidgetId {
        let count = self.counters.entry(raw_id).or_insert(0);
        let final_id = if *count == 0 {
            raw_id
        } else {
            raw_id.with(*count)
        };
        let was_collision = *count > 0;
        *count += 1;
        if was_collision && is_explicit {
            self.pending.push(PendingExplicitCollision {
                first_raw_id: raw_id,
                second_final_id: final_id,
            });
        }
        final_id
    }

    /// Record the endpoint where `final_id` is being opened. Returns
    /// any [`PendingExplicitCollision`] queued at [`Self::resolve`]
    /// for this final id paired with the first occurrence's
    /// endpoint, so the caller can push a `CollisionRecord` —
    /// emitted to `Forest.collisions` for the magenta overlay.
    ///
    /// Debug-asserts the `curr` slot is vacant. Pathological inputs
    /// like `.id(X)`, `.id(X)`, `.id(X.with(1))` would otherwise let
    /// the third widget overwrite the second's endpoint; the assert
    /// catches it loudly in dev builds without the cost of a
    /// disambiguation loop here on the hot path.
    pub(crate) fn record_endpoint(
        &mut self,
        final_id: WidgetId,
        endpoint: Endpoint,
    ) -> Option<(Endpoint, Endpoint)> {
        debug_assert!(
            !self.curr.contains_key(&final_id),
            "record_endpoint called twice for {final_id:?} — caller likely passed an explicit `.id(X.with(N))` that collides with a disambiguated auto/explicit slot",
        );
        self.curr.insert(final_id, endpoint);
        let idx = self
            .pending
            .iter()
            .position(|p| p.second_final_id == final_id)?;
        let pending = self.pending.swap_remove(idx);
        let first = self.curr.get(&pending.first_raw_id).copied()?;
        Some((first, endpoint))
    }

    /// Populate `self.removed` with widgets present in `prev` but
    /// absent from `curr`, then swap `curr → prev` so the next frame
    /// diffs against this one. Returns a borrow of `self.removed`
    /// for callers that want to fan the diff straight into per-widget
    /// caches (text shaper, measure cache, state map, animation,
    /// damage); the field stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &FxHashSet<WidgetId> {
        self.removed.clear();
        for wid in self.prev.keys() {
            if !self.curr.contains_key(wid) {
                self.removed.insert(*wid);
            }
        }
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        &self.removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forest::tree::NodeId;

    fn ep(node: u32) -> Endpoint {
        Endpoint {
            layer: Layer::Main,
            node: NodeId(node),
        }
    }

    #[test]
    fn resolve_returns_raw_id_on_first_call() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        assert_eq!(ids.resolve(x, false), x);
        assert_eq!(ids.counters[&x], 1);
    }

    #[test]
    fn resolve_disambiguates_collisions_by_occurrence() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        assert_eq!(ids.resolve(x, false), x);
        assert_eq!(ids.resolve(x, false), x.with(1));
        assert_eq!(ids.resolve(x, false), x.with(2));
    }

    #[test]
    fn resolve_queues_pending_only_for_explicit_collisions() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        ids.resolve(x, false);
        ids.resolve(x, false); // auto collision — silent
        assert!(ids.pending.is_empty());

        let y = WidgetId::from_hash("y");
        ids.resolve(y, true);
        ids.resolve(y, true); // explicit collision — queued
        assert_eq!(ids.pending.len(), 1);
        assert_eq!(ids.pending[0].first_raw_id, y);
        assert_eq!(ids.pending[0].second_final_id, y.with(1));
    }

    #[test]
    fn record_endpoint_emits_collision_pair_for_explicit_only() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        // First occurrence resolves + opens.
        let first = ids.resolve(x, true);
        assert_eq!(ids.record_endpoint(first, ep(1)), None);
        // Second occurrence resolves + opens — should hand back the pair.
        let second = ids.resolve(x, true);
        let pair = ids.record_endpoint(second, ep(2)).expect("collision pair");
        assert_eq!(pair, (ep(1), ep(2)));
        // Pending drained.
        assert!(ids.pending.is_empty());
    }

    #[test]
    fn record_endpoint_no_pair_for_auto_collisions() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        let first = ids.resolve(x, false);
        ids.record_endpoint(first, ep(1));
        let second = ids.resolve(x, false);
        assert_eq!(ids.record_endpoint(second, ep(2)), None);
    }

    #[test]
    fn pre_record_clears_per_frame_state_but_keeps_prev() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        ids.resolve(x, false);
        ids.record_endpoint(x, ep(1));
        ids.rollover();
        assert!(ids.curr.is_empty());
        assert_eq!(ids.prev.len(), 1);

        // Counters and pending persist across rollover (intentionally —
        // rollover is the painted-frame swap, not the frame boundary).
        // `pre_record` is what clears per-frame disambiguation state at
        // the start of the next record cycle.
        assert_eq!(ids.counters[&x], 1);

        ids.pre_record();
        assert!(ids.counters.is_empty());
        assert!(ids.curr.is_empty());
        assert_eq!(ids.prev.len(), 1, "prev must survive pre_record");
    }
}
