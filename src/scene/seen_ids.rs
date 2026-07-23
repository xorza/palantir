//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Eager disambiguation.** [`Self::resolve`] runs at
//!    `Ui::widget` time — *before* the matching `Widget::record`
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
//!    `Forest::open_node` time, after the final id has been carried
//!    there by the `Widget`. Stores `final_id → Endpoint` so
//!    the magenta debug overlay has both halves of a collision pair
//!    on hand.
//! 3. **Removed-widget diff + rollover.** [`Self::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::scene::damage::DamageEngine`] /
//!    [`crate::text::TextShaper`] / measure cache / state /
//!    animation), then swaps `curr → prev` so the next frame diffs
//!    against this one. Called once per application frame from
//!    [`crate::Ui::finalize_frame`]; `prev` stays anchored at the last
//!    *painted* frame regardless of how many discard passes ran. Ids
//!    seen only in a discarded pass (double-layout pass A, cold-start
//!    warmup) are collected into `discarded` at the next `pre_record`
//!    and folded into `removed` — they reach neither `prev` nor the
//!    final `curr`, and without the fold their state/anim/text rows
//!    would leak and resume stale if the widget later reappeared.

use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::layer::Layer;
use crate::scene::tree::node::NodeId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;

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

#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: Endpoint,
    pub(crate) second: Endpoint,
}

/// One side of a queued explicit-collision pair. The first endpoint
/// is looked up by `first_raw_id` (the un-disambiguated id of the
/// first occurrence, already recorded in `curr` when this entry is
/// queued); the second endpoint is filled in at
/// [`SeenIds::record_endpoint`] when `second_final_id` is opened.
#[derive(Clone, Copy, Debug)]
struct PendingExplicitCollision {
    first_raw_id: WidgetId,
    second_final_id: WidgetId,
}

/// Outcome of [`SeenIds::record_endpoint`] — either the endpoint
/// recorded cleanly, or it completed a queued explicit-collision
/// pair (caller pushes a [`CollisionRecord`] from the
/// two endpoints).
#[derive(Clone, Copy, Debug)]
pub(crate) enum EndpointOutcome {
    /// No collision — id was unique this frame (or it was a silent
    /// auto-collision whose endpoint just got filed away in `curr`).
    Recorded,
    /// The endpoint just recorded completed a pending explicit
    /// collision. `first` is where the un-disambiguated id was
    /// opened earlier this frame; `second` is the endpoint passed
    /// to this `record_endpoint` call.
    ExplicitCollision { first: Endpoint, second: Endpoint },
}

#[derive(Debug, Default)]
pub(crate) struct SeenIds {
    /// Per-raw-id occurrence counter. Bumped inside [`Self::resolve`]
    /// when the raw id is already occupied. Candidate ids normally
    /// progress through `raw_id.with(1)`, `.with(2)`, etc.; explicitly
    /// occupied candidates are skipped. Cleared each frame in
    /// [`Self::pre_record`]. Independent of the `(layer, node)` of the
    /// actual record, so `Ui::widget` resolves the right id before any
    /// node exists.
    counters: FxHashMap<WidgetId, u32>,
    /// `final_id → Endpoint` of every widget actually opened this
    /// frame. Populated by [`Self::record_endpoint`] from
    /// `Forest::open_node`. Read for explicit-collision endpoint
    /// resolution (the first endpoint lives under `raw_id`, which is
    /// the un-disambiguated form of any subsequent occurrence). Same
    /// keys feed the [`Self::rollover`] removed-diff and the
    /// [`crate::scene::cascade::Cascades::by_id`] snapshot taken at
    /// the end of each `CascadesEngine::run`.
    pub(crate) curr: WidgetIdMap<Endpoint>,
    /// Last *painted* frame's `curr`. Only the keys matter for the
    /// rollover diff — values are stale across frames. Same type as
    /// `curr` so `std::mem::swap` is alloc-free.
    prev: WidgetIdMap<Endpoint>,
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
    /// Ids recorded by a pass this frame that was then discarded —
    /// drained from `curr` by the next `pre_record` of the same
    /// frame. Folded into `removed` at [`Self::rollover`] (unless
    /// re-recorded by the final pass) so rows created during a
    /// discarded pass don't leak. Capacity retained.
    discarded: FxHashSet<WidgetId>,
}

impl SeenIds {
    /// Reset per-frame state at the top of a record pass. Clears the
    /// `curr` recording map + the disambiguation counter + pending
    /// collisions. **Doesn't touch `prev`** — that holds the last
    /// *painted* frame's recording, established by [`Self::rollover`].
    /// A two-pass frame calls `pre_record` then
    /// never reaches `rollover`, so `prev` must be preserved across
    /// the discard. A non-empty `curr` here IS such a discarded pass
    /// (rollover empties it at frame end) — its ids move to
    /// `discarded` so rows they created can be swept if the final
    /// pass drops them.
    pub(crate) fn pre_record(&mut self) {
        self.counters.clear();
        self.discarded.extend(self.curr.keys().copied());
        self.curr.clear();
        self.pending.clear();
    }

    /// Eagerly resolve a raw id to its disambiguated final id.
    /// Common case (first occurrence of `raw_id` this frame) hits a
    /// single `curr.contains_key` probe and returns `raw_id`
    /// unchanged — `counters` stays untouched. Collision case advances
    /// the per-raw-id counter until `raw_id.with(count)` is vacant.
    /// Explicit collisions queue a [`PendingExplicitCollision`] so
    /// [`Self::record_endpoint`] can emit the magenta-overlay
    /// [`CollisionRecord`] once both endpoints exist.
    ///
    /// **Contract**: the matching [`Self::record_endpoint`] for an
    /// earlier `resolve(raw_id)` must run before the next
    /// `resolve(raw_id)` — otherwise this routine can't see the
    /// first occurrence in `curr` and would incorrectly report
    /// "first time". Widget call sites pair them immediately
    /// (`Ui::widget` → `Widget::record` → `scene::open_node`),
    /// so the contract holds for production code.
    #[inline]
    pub(crate) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> WidgetId {
        if !self.curr.contains_key(&raw_id) {
            // Fast path — first occurrence. `counters` only tracks
            // raw ids that actually collided, so its size is
            // `collisions / frame` (typically 0), not
            // `widgets / frame`.
            return raw_id;
        }
        let (counters, curr) = (&mut self.counters, &self.curr);
        let count = counters.entry(raw_id).or_insert(0);
        let final_id = loop {
            *count = count
                .checked_add(1)
                .expect("WidgetId occurrence counter overflowed");
            let candidate = raw_id.with(*count);
            if !curr.contains_key(&candidate) {
                break candidate;
            }
        };
        if is_explicit {
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
    /// Panics if the `curr` slot is occupied. [`Self::resolve`] must
    /// return an available id, and using the entry API enforces that
    /// invariant without overwriting the existing endpoint.
    #[inline]
    pub(crate) fn record_endpoint(
        &mut self,
        final_id: WidgetId,
        endpoint: Endpoint,
    ) -> EndpointOutcome {
        let Entry::Vacant(entry) = self.curr.entry(final_id) else {
            panic!("record_endpoint called twice for {final_id:?}");
        };
        entry.insert(endpoint);
        let Some(idx) = self
            .pending
            .iter()
            .position(|p| p.second_final_id == final_id)
        else {
            return EndpointOutcome::Recorded;
        };
        let pending = self.pending.swap_remove(idx);
        // First occurrence's endpoint is filed under the
        // un-disambiguated raw id and MUST already be present:
        // `resolve` only queues a pending entry on the *second*
        // explicit `resolve(X, true)` call this frame, and widgets
        // pair `Ui::widget` with an immediate `Widget::record` left-
        // to-right, so the first widget's `record_endpoint(X, ...)`
        // always runs before the second's. A missing entry means the
        // recording-order contract was violated — surface loudly.
        let first = self
            .curr
            .get(&pending.first_raw_id)
            .copied()
            .expect("pending explicit collision references a raw id whose first endpoint hasn't been recorded — recording order violated");
        EndpointOutcome::ExplicitCollision {
            first,
            second: endpoint,
        }
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
        // Ids seen only in a discarded pass this frame (double-layout
        // pass A, cold-start warmup) are in neither `prev` nor `curr`
        // — the prev-minus-curr diff can't see them, but any state /
        // anim / measure / text rows they created during that pass are
        // real and must be swept with everything else.
        for wid in self.discarded.iter() {
            if !self.curr.contains_key(wid) {
                self.removed.insert(*wid);
            }
        }
        self.discarded.clear();
        std::mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        &self.removed
    }
}

#[cfg(test)]
mod tests {
    use crate::scene::seen_ids::*;
    use crate::scene::tree::node::NodeId;

    fn ep(node: u32) -> Endpoint {
        Endpoint {
            layer: Layer::Main,
            node: NodeId(node),
        }
    }

    /// Stand-in for the production `resolve → record_endpoint`
    /// pairing every widget does (`Ui::widget` →
    /// `scene::open_node`). The lazy-counter fast path in `resolve`
    /// depends on `curr` being populated between consecutive resolves
    /// of the same raw id, so tests interleave them the same way.
    fn open(ids: &mut SeenIds, raw_id: WidgetId, is_explicit: bool, node: u32) -> WidgetId {
        let final_id = ids.resolve(raw_id, is_explicit);
        ids.record_endpoint(final_id, ep(node));
        final_id
    }

    #[test]
    fn resolve_returns_raw_id_on_first_call() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        assert_eq!(open(&mut ids, x, false, 1), x);
        // Fast path didn't touch `counters` — only collisions populate it.
        assert!(ids.counters.is_empty());
    }

    #[test]
    fn resolve_disambiguates_collisions_by_occurrence() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        assert_eq!(open(&mut ids, x, false, 1), x);
        assert_eq!(open(&mut ids, x, false, 2), x.with(1));
        assert_eq!(open(&mut ids, x, false, 3), x.with(2));
    }

    #[test]
    fn resolve_skips_occupied_occurrence_ids() {
        let x = WidgetId::from_hash("x");

        for occupied_slots in [1_u32, 2] {
            let mut ids = SeenIds::default();
            assert_eq!(open(&mut ids, x, true, 0), x);

            for slot in 1..=occupied_slots {
                let occupied = x.with(slot);
                assert_eq!(open(&mut ids, occupied, true, slot), occupied);
            }

            let node = occupied_slots + 1;
            let final_id = open(&mut ids, x, true, node);
            assert_eq!(final_id, x.with(occupied_slots + 1));
            assert_eq!(ids.curr.len(), (occupied_slots + 2) as usize);
            assert_eq!(ids.curr[&x], ep(0));
            for slot in 1..=occupied_slots {
                assert_eq!(ids.curr[&x.with(slot)], ep(slot));
            }
            assert_eq!(ids.curr[&final_id], ep(node));
            assert!(ids.pending.is_empty());
        }
    }

    #[test]
    fn resolve_queues_pending_only_for_explicit_collisions() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        open(&mut ids, x, false, 1);
        open(&mut ids, x, false, 2); // auto collision — silent
        assert!(ids.pending.is_empty());

        let y = WidgetId::from_hash("y");
        // First explicit — fast path, no pending.
        ids.resolve(y, true);
        ids.record_endpoint(y, ep(3));
        // Second explicit — collision, queued. record_endpoint will
        // drain it; check it was queued first.
        let second = ids.resolve(y, true);
        assert_eq!(ids.pending.len(), 1);
        assert_eq!(ids.pending[0].first_raw_id, y);
        assert_eq!(ids.pending[0].second_final_id, second);
    }

    #[test]
    fn record_endpoint_emits_collision_pair_for_explicit_only() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        // First occurrence resolves + opens.
        let first = ids.resolve(x, true);
        assert!(matches!(
            ids.record_endpoint(first, ep(1)),
            EndpointOutcome::Recorded
        ));
        // Second occurrence resolves + opens — should hand back the pair.
        let second = ids.resolve(x, true);
        match ids.record_endpoint(second, ep(2)) {
            EndpointOutcome::ExplicitCollision { first, second } => {
                assert_eq!(first, ep(1));
                assert_eq!(second, ep(2));
            }
            EndpointOutcome::Recorded => panic!("expected collision pair"),
        }
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
        assert!(matches!(
            ids.record_endpoint(second, ep(2)),
            EndpointOutcome::Recorded
        ));
    }

    #[test]
    fn record_endpoint_rejects_duplicate_without_overwriting() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        ids.record_endpoint(x, ep(1));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ids.record_endpoint(x, ep(2));
        }));

        assert!(result.is_err());
        assert_eq!(ids.curr[&x], ep(1));
    }

    #[test]
    #[should_panic(expected = "recording order violated")]
    fn record_endpoint_panics_if_first_endpoint_missing() {
        // Manually queue a pending collision whose first raw id was
        // never recorded — bypasses the production resolve+record
        // pairing to simulate the contract violation. The expect in
        // `record_endpoint` must fire — the alternative is a silent
        // miss that hides a recording-order bug from the magenta
        // collision overlay.
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        let second = x.with(1);
        ids.pending.push(PendingExplicitCollision {
            first_raw_id: x,
            second_final_id: second,
        });
        ids.record_endpoint(second, ep(2));
    }

    #[test]
    fn rollover_sweeps_ids_seen_only_in_a_discarded_pass() {
        let mut ids = SeenIds::default();
        let a = WidgetId::from_hash("a");
        let b = WidgetId::from_hash("b");
        // Pass A records a + b, then is discarded by the next
        // pre_record (double-layout / warmup shape).
        open(&mut ids, a, false, 1);
        open(&mut ids, b, false, 2);
        ids.pre_record();
        // Final pass records only a.
        open(&mut ids, a, false, 1);
        let removed = ids.rollover();
        assert!(
            removed.contains(&b),
            "pass-A-only id must be swept or its state rows leak"
        );
        assert!(
            !removed.contains(&a),
            "id re-recorded in the final pass survives"
        );
        // The discarded set drained at rollover: the next frame's diff
        // doesn't resurrect b.
        ids.pre_record();
        open(&mut ids, a, false, 1);
        let removed = ids.rollover();
        assert!(removed.is_empty(), "got {removed:?}");
    }

    #[test]
    fn pre_record_clears_per_frame_state_but_keeps_prev() {
        let mut ids = SeenIds::default();
        let x = WidgetId::from_hash("x");
        // Force `counters` to be non-empty by opening the same id
        // twice (collision path populates it).
        open(&mut ids, x, false, 1);
        open(&mut ids, x, false, 2);
        assert!(!ids.counters.is_empty());

        ids.rollover();
        assert!(ids.curr.is_empty());
        assert_eq!(ids.prev.len(), 2);
        // Counters persist across rollover (rollover is the painted-
        // frame swap; `pre_record` clears per-frame disambiguation
        // state at the next record cycle).
        assert!(!ids.counters.is_empty());

        ids.pre_record();
        assert!(ids.counters.is_empty());
        assert!(ids.curr.is_empty());
        assert_eq!(ids.prev.len(), 2, "prev must survive pre_record");
    }
}
