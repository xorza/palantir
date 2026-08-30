//! Per-frame `WidgetId` tracker. Owns three things that all key off
//! "which widgets were recorded this frame":
//!
//! 1. **Eager disambiguation.** [`SeenIds::resolve`] runs at
//!    `Ui::widget` time — *before* the matching `Widget::record`
//!    opens the actual record. It rewrites the resolved id by mixing
//!    in an occurrence counter when the raw id has already been
//!    handed out this frame, so the returned id matches what the
//!    tree, cascade, and `response_for` will see. Per-id state
//!    (focus, scroll, capture, hit-test) stays positional within the
//!    colliding call site. Explicit-key collisions (`.id(X)`,
//!    `.id_salt(X)`) are caller bugs: `resolve` queues a
//!    [`PendingExplicitCollision`] for the second occurrence and
//!    [`SeenIds::record_endpoint`] finalizes the [`CollisionRecord`]
//!    once both opens have provided their `Endpoint`s.
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

use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::endpoint::Endpoint;
use rustc_hash::{FxHashMap, FxHashSet};
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
    /// actual record, so `Ui::widget` resolves the right id before any
    /// node exists.
    counters: FxHashMap<WidgetId, u32>,
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
    pub(crate) removed: FxHashSet<WidgetId>,
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
        // Scanned rather than mapped: an explicit collision is a caller
        // bug, so `pending` is empty on the frames that matter and this
        // is a length test — where a hash probe would cost every node of
        // every frame.
        let idx = self
            .pending
            .iter()
            .position(|p| p.second_final_id == final_id)?;
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
        Some(CollisionRecord {
            first,
            second: endpoint,
        })
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
mod tests;
