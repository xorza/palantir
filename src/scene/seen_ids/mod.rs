//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Eager disambiguation.** [`SeenIds::resolve`] runs at
//!    `Widget::resolve` time — *before* the matching `Widget::record`
//!    opens the actual record. It rewrites the resolved id by mixing
//!    in an occurrence counter when the raw id has already been
//!    handed out this frame, so the returned id matches what the
//!    tree, cascade, and `response_for` will see. Per-id state
//!    (focus, scroll, capture, hit-test) stays positional within the
//!    colliding call site. Explicit-key collisions (`.id(X)`,
//!    `.id_salt(X)`) are caller bugs: `resolve` queues a
//!    [`PendingExplicitCollision`] for the second occurrence and
//!    [`SeenIds::record_endpoint`] finalizes the [`CollisionRecord`]
//!    once both opens have provided their `Endpoint`s. An id counts as
//!    taken from the moment `resolve` hands it out, so two widgets that
//!    both resolve before either records still get distinct ids.
//! 2. **Endpoint tracking.** [`SeenIds::record_endpoint`] runs at
//!    `Forest::open_node` time, after the final id has been carried
//!    there by the `Widget`. Stores `final_id → Endpoint` so
//!    the magenta debug overlay has both halves of a collision pair
//!    on hand.
//! 3. **Removed-widget diff + rollover.** [`SeenIds::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::scene::damage::DamageEngine`] /
//!    [`crate::text::shaper::TextShaper`] / measure cache / state /
//!    animation), then swaps `curr → prev` so the next frame diffs
//!    against this one. Called once per application frame from
//!    `FrameCycle::finalize_frame`; `prev` stays anchored at the last
//!    *painted* frame regardless of how many discard passes ran. Ids
//!    seen only in a discarded pass (double-layout pass A, cold-start
//!    warmup) are collected into `discarded` at the next `pre_record`
//!    and folded into `removed` — they reach neither `prev` nor the
//!    final `curr`, and without the fold their state/anim/text rows
//!    would leak and resume stale if the widget later reappeared.

use crate::primitives::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::scene::endpoint::Endpoint;
use std::collections::hash_map::Entry;

/// Both nodes of one explicit-id collision, in recording order. What
/// [`SeenIds::record_endpoint`] hands back when the endpoint it just
/// filed completed a pair. Logged by `Forest` in every profile, then
/// accumulated into `Forest.collisions` for `encoder::collision_overlay`
/// (`debug_assertions`) and `UiHarness::collisions` (`internals`).
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

#[derive(Debug, Default)]
pub(crate) struct SeenIds {
    /// Per-raw-id occurrence counter. Bumped inside [`Self::resolve`]
    /// when the raw id is already occupied. Candidate ids normally
    /// progress through `raw_id.with(1)`, `.with(2)`, etc.; explicitly
    /// occupied candidates are skipped. Cleared each frame in
    /// [`Self::pre_record`]. Independent of the `(layer, node)` of the
    /// actual record, so `Widget::resolve` answers the right id before any
    /// node exists.
    counters: WidgetIdMap<u32>,
    /// `final_id → Endpoint` of every widget actually opened this
    /// frame. Populated by [`Self::record_endpoint`] from
    /// `Forest::open_node`. Read for explicit-collision endpoint
    /// resolution (the first endpoint lives under `raw_id`, which is
    /// the un-disambiguated form of any subsequent occurrence). Same
    /// keys feed the [`Self::rollover`] removed-diff and the
    /// [`crate::scene::cascade::Cascade::by_id`] snapshot taken at
    /// the end of each `CascadeEngine::run`.
    pub(crate) curr: WidgetIdMap<Endpoint>,
    /// Last *painted* frame's `curr`. Only the keys matter for the
    /// rollover diff — values are stale across frames. Same type as
    /// `curr` so `std::mem::swap` is alloc-free.
    ///
    /// [`Cascade::by_id`](crate::scene::cascade::Cascade) holds the same
    /// entries with live values and still cannot serve this diff: it is
    /// refreshed at every cascade *run*, so a two-pass frame overwrites
    /// it before rollover and the diff would compare pass A against
    /// pass B rather than frame against frame.
    prev: WidgetIdMap<Endpoint>,
    /// Diff output: widgets present in `prev` but not in `curr`.
    /// Repopulated by [`Self::rollover`]; consumers iterate via a
    /// shared borrow on the field. Public-in-crate so callers can
    /// hold `&seen.removed` across other shared `&forest` reads — an
    /// accessor returning `&[..]` would tie the returned slice to the
    /// `&mut self` and block those reads.
    pub(crate) removed: WidgetIdSet,
    /// Explicit collisions queued by [`Self::resolve`] awaiting
    /// endpoint resolution at [`Self::record_endpoint`]. Each entry
    /// names the first occurrence's raw id (whose endpoint is already
    /// in `curr`) and the second occurrence's final id (whose
    /// endpoint arrives when `record_endpoint` opens it). Cleared
    /// each frame.
    pending: Vec<PendingExplicitCollision>,
    /// Ids that exist *only* inside this frame's discarded passes —
    /// recorded by a pass that was then thrown away, and absent from
    /// `prev`. Drained from `curr` by the next `pre_record` of the same
    /// frame, folded into `removed` at [`Self::rollover`] (unless
    /// re-recorded by the final pass) so rows created during a discarded
    /// pass don't leak. Capacity retained.
    ///
    /// A discarded id that `prev` also holds stays out: the
    /// prev-minus-curr diff already reports it. That filter is what keeps
    /// this empty on the frames that matter — a settling second pass over
    /// a thousand steady widgets adds nothing here instead of a thousand
    /// entries.
    discarded: WidgetIdSet,
    /// Ids [`Self::resolve`] handed out this pass that no
    /// [`Self::record_endpoint`] has filed yet. A widget may resolve
    /// before it records, and another may resolve the same raw id in
    /// between — `.state(ui)` on two auto-id buttons from one call site,
    /// then `.show()` on both. Probing `curr` alone, the second would get
    /// the first's id and its open would hit the duplicate panic; probing
    /// this set too, it is disambiguated. Auto ids only: an explicit id
    /// found here is its owner re-resolving it (see [`Self::resolve`]).
    /// Holds about one id at a time on the usual resolve-then-record
    /// path, and is cleared per pass, so an id resolved and never shown
    /// occupies its occurrence for that pass only.
    reserved: WidgetIdSet,
}

impl SeenIds {
    /// Reset per-frame state at the top of a record pass. Clears the
    /// `curr` recording map + the disambiguation counter + pending
    /// collisions. **Doesn't touch `prev`** — that holds the last
    /// *painted* frame's recording, established by [`Self::rollover`].
    /// A two-pass frame calls `pre_record` then
    /// never reaches `rollover`, so `prev` must be preserved across
    /// the discard. A non-empty `curr` here IS such a discarded pass
    /// (rollover empties it at frame end) — the ids it holds that `prev`
    /// has never seen move to `discarded`, so rows they created can be
    /// swept if the final pass drops them. The ones `prev`
    /// does hold need no help: [`Self::rollover`]'s prev-minus-curr diff
    /// reports exactly those, so copying them here would be a hash insert
    /// per widget per settling pass to restate what the diff already says.
    pub(crate) fn pre_record(&mut self) {
        self.counters.clear();
        self.discarded.extend(
            self.curr
                .keys()
                .filter(|wid| !self.prev.contains_key(*wid))
                .copied(),
        );
        self.curr.clear();
        self.pending.clear();
        self.reserved.clear();
    }

    /// Whether `id` is taken this pass — recorded, or handed out by
    /// [`Self::resolve`] and not yet recorded.
    #[inline]
    fn occupied(&self, id: WidgetId) -> bool {
        self.curr.contains_key(&id) || self.reserved.contains(&id)
    }

    /// Eagerly resolve a raw id to its disambiguated final id, and
    /// reserve it until its [`Self::record_endpoint`].
    /// Common case (first occurrence of `raw_id` this pass) returns
    /// `raw_id` unchanged — `counters` stays untouched. Collision case
    /// advances the per-raw-id counter until `raw_id.with(count)` is
    /// neither recorded nor reserved. Explicit collisions queue a
    /// [`PendingExplicitCollision`] so [`Self::record_endpoint`] can emit
    /// the magenta-overlay [`CollisionRecord`] once both endpoints exist,
    /// in whichever order the two record.
    #[inline]
    pub(crate) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> WidgetId {
        // An explicit id that is only reserved is the widget that
        // reserved it, claiming it: a widget resolves its own id, then
        // records a wrapper under `.id(resolved)`. Only an auto id, which
        // two call-site twins can share, disambiguates against a
        // reservation.
        let taken =
            self.curr.contains_key(&raw_id) || (!is_explicit && self.reserved.contains(&raw_id));
        if !taken {
            // Fast path — first occurrence. `counters` only tracks
            // raw ids that actually collided, so its size is
            // `collisions / frame` (typically 0), not
            // `widgets / frame`.
            self.reserved.insert(raw_id);
            return raw_id;
        }
        let mut count = self.counters.get(&raw_id).copied().unwrap_or(0);
        let final_id = loop {
            count = count
                .checked_add(1)
                .expect("WidgetId occurrence counter overflowed");
            let candidate = raw_id.with(count);
            if !self.occupied(candidate) {
                break candidate;
            }
        };
        self.counters.insert(raw_id, count);
        self.reserved.insert(final_id);
        if is_explicit {
            self.pending.push(PendingExplicitCollision {
                first_raw_id: raw_id,
                second_final_id: final_id,
            });
        }
        final_id
    }

    /// Record the endpoint where `final_id` is being opened. `Some`
    /// when this endpoint completed a [`PendingExplicitCollision`]
    /// queued at [`Self::resolve`], pairing it with the first
    /// occurrence's endpoint — `None` on every other open, which is the
    /// common case for every node of every frame.
    ///
    /// Panics if the `curr` slot is occupied. [`Self::resolve`] must
    /// return an available id, and using the entry API enforces that
    /// invariant without overwriting the existing endpoint.
    #[inline]
    pub(crate) fn record_endpoint(
        &mut self,
        final_id: WidgetId,
        endpoint: Endpoint,
    ) -> Option<CollisionRecord> {
        let Entry::Vacant(entry) = self.curr.entry(final_id) else {
            panic!("record_endpoint called twice for {final_id:?}");
        };
        entry.insert(endpoint);
        self.reserved.remove(&final_id);
        // Scanned rather than mapped: an explicit collision is a caller
        // bug, so `pending` is empty on the frames that matter and this
        // is a length test — where a hash probe would cost every node of
        // every frame.
        //
        // Either side may record last: a widget that resolved before
        // recording lets its duplicate open first. The pair completes
        // on whichever record supplies the second endpoint.
        let idx = self.pending.iter().position(|p| {
            (p.second_final_id == final_id || p.first_raw_id == final_id)
                && self.curr.contains_key(&p.first_raw_id)
                && self.curr.contains_key(&p.second_final_id)
        })?;
        let pending = self.pending.swap_remove(idx);
        Some(CollisionRecord {
            first: self.curr[&pending.first_raw_id],
            second: self.curr[&pending.second_final_id],
        })
    }

    /// Populate `self.removed` with widgets present in `prev` but
    /// absent from `curr`, then swap `curr → prev` so the next frame
    /// diffs against this one. Returns a borrow of `self.removed`
    /// for callers that want to fan the diff straight into per-widget
    /// caches (text shaper, measure cache, state map, animation,
    /// damage); the field stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &WidgetIdSet {
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
pub(crate) mod test_support {
    use crate::primitives::widget_id::WidgetIdMap;
    use crate::scene::endpoint::Endpoint;
    use crate::scene::seen_ids::SeenIds;

    impl SeenIds {
        /// The ids the last finished frame recorded — `curr` until
        /// `rollover`, `prev` after it.
        pub(crate) fn last_frame(&self) -> &WidgetIdMap<Endpoint> {
            &self.prev
        }
    }
}

#[cfg(test)]
mod tests;
