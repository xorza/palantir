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
//!    taken from the moment `resolve` hands it out — it enters `curr` as
//!    [`IdSlot::Reserved`] — so two widgets that both resolve before
//!    either records still get distinct ids. The [`ResolvedId`] it
//!    returns carries the entry's index, so the record writes the entry
//!    without a second hash probe.
//! 2. **Endpoint tracking.** [`SeenIds::record_endpoint`] runs at
//!    `Forest::open_node` time, after the final id has been carried
//!    there by the `Widget`. Turns the id's slot into
//!    [`IdSlot::Recorded`] so the magenta debug overlay has both halves
//!    of a collision pair on hand.
//! 3. **Removed-widget diff + rollover.** [`SeenIds::rollover`] computes
//!    which ids were present last painted frame but absent this pass
//!    (populating `removed` for [`crate::damage::engine::DamageEngine`] /
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
//!
//! **In step with the last frame.** A frame usually resolves the same raw
//! ids in the same order as the one before it. While it does, each id is
//! known without a probe: an id that took its raw id last frame cannot be
//! taken yet, because last frame's ids are distinct and this pass holds
//! only the ones before it; and an auto id that collided gets the same
//! occurrence, from the same taken set. So `curr` builds no hash index
//! until the first resolve that differs, then indexes the entries so far,
//! sets the counters from them, and goes on probing, as every resolve did
//! before. The rollover diff compares by position over the matching
//! prefix. A frame that changes early costs what every frame cost without
//! this, and a steady frame hashes no id. Ordered reconciliation does the
//! same: Flutter's `updateChildren` and the Vue and Inferno diffs walk the
//! old and the new children in order and fall back to a keyed map only
//! past a mismatch.

use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::scene::endpoint::Endpoint;
use crate::scene::node::ident::Ident;
use std::collections::hash_map::Entry;
use std::mem;
use std::ptr;

/// Both nodes of one explicit-id collision, in recording order. What
/// [`SeenIds::record_endpoint`] hands back when the endpoint it just
/// filed completed a pair. Logged by `Forest` in every profile, then
/// accumulated into `Forest.collisions` for `encoder::collision_overlay`
/// (`debug_assertions`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: Endpoint,
    pub(crate) second: Endpoint,
}

/// What one id holds in [`SeenIds::curr`] this pass.
///
/// One table for both states, because every widget resolves then
/// records: resolve pays one probe, and record none — it writes through
/// the index the [`ResolvedId`] carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdSlot {
    /// Handed out by [`SeenIds::resolve`], not yet recorded. A widget may
    /// resolve before it records, and another may resolve the same raw id
    /// in between — `.state(ui)` on two auto-id buttons from one call
    /// site, then `.show()` on both — so the id must count as taken from
    /// here. Lasts one pass: an id resolved and never shown occupies its
    /// occurrence for that pass only, and is in no frame's recording.
    Reserved,
    /// Opened at this endpoint.
    Recorded(Endpoint),
}

impl IdSlot {
    #[inline]
    const fn endpoint(self) -> Option<Endpoint> {
        match self {
            Self::Reserved => None,
            Self::Recorded(endpoint) => Some(endpoint),
        }
    }
}

/// An id [`SeenIds::resolve`] handed out this pass, with the index of
/// the entry that reserves it. Only `resolve` makes one, so the index
/// always names that entry; [`SeenIds::record_endpoint`] checks the id
/// against it all the same, since a stale value from an earlier pass
/// would otherwise write another widget's entry.
///
/// Packed to 4-byte alignment so it fits the 8 bytes beside
/// [`Ident`](crate::scene::node::ident::Ident)'s tag, which every
/// widget carries: aligned to 8 it is 16 bytes, and each widget grows by
/// 8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(Rust, packed(4))]
pub(crate) struct ResolvedId {
    id: WidgetId,
    entry: u32,
}

impl ResolvedId {
    #[inline]
    pub(crate) const fn id(self) -> WidgetId {
        self.id
    }
}

/// One id handed out this pass, and what it holds.
#[derive(Clone, Copy, Debug)]
struct IdEntry {
    id: WidgetId,
    slot: IdSlot,
    origin: Origin,
}

/// How a pass came to an entry's id: what the next frame's resolve at the
/// same position must repeat to stay in step.
#[derive(Clone, Copy, Debug)]
struct Origin {
    /// The raw id that resolved to the entry's id, and whether it was
    /// explicit.
    raw: WidgetId,
    is_explicit: bool,
    /// The occurrence counter the id took, or 0 when the id is `raw`.
    occurrence: u32,
    /// The inputs of a parent-scoped `raw`, which name it without a hash.
    recipe: Option<Recipe>,
}

/// A parent-scoped raw id before its hash: an auto id's call site or a
/// salt, and the parent it hangs off.
#[derive(Clone, Copy, Debug)]
struct Recipe {
    /// [`Ident::Auto`] or [`Ident::Hash`].
    ident: Ident,
    parent: Option<WidgetId>,
}

impl Recipe {
    /// Whether `other` names the same raw id by the same inputs. A
    /// `Location` is compared by address: a call site the compiler
    /// duplicated only fails to match, and the resolve then hashes it.
    #[inline]
    fn is(self, other: Self) -> bool {
        let same = match (self.ident, other.ident) {
            (Ident::Auto(a), Ident::Auto(b)) => ptr::eq(a, b),
            (Ident::Hash(a), Ident::Hash(b)) => a == b,
            _ => false,
        };
        same && self.parent == other.parent
    }
}

/// One pass's ids: a hash index over a dense run of entries. The index
/// answers "is this id taken" at resolve; the run is what a record
/// writes through, and what the diffs and the cascade's id snapshot
/// walk, in resolve order. `curr`'s index is empty while the pass is in
/// step with `prev`, and `prev`'s always covers every entry.
#[derive(Debug, Default)]
struct IdTable {
    index: WidgetIdMap<u32>,
    entries: Vec<IdEntry>,
}

impl IdTable {
    #[inline]
    fn slot(&self, id: WidgetId) -> Option<IdSlot> {
        self.index
            .get(&id)
            .map(|&entry| self.entries[entry as usize].slot)
    }

    /// Whether `id` is held as recorded, not only reserved.
    #[inline]
    fn recorded(&self, id: WidgetId) -> bool {
        self.slot(id).and_then(IdSlot::endpoint).is_some()
    }

    /// The recorded ids, with their endpoints, in resolve order.
    #[inline]
    fn recorded_entries(&self) -> impl Iterator<Item = (WidgetId, Endpoint)> + '_ {
        self.entries
            .iter()
            .filter_map(|entry| Some((entry.id, entry.slot.endpoint()?)))
    }

    fn clear(&mut self) {
        self.index.clear();
        self.entries.clear();
    }

    /// Index every entry, into an empty index.
    fn index_entries(&mut self) {
        debug_assert!(self.index.is_empty());
        self.index.extend(
            self.entries
                .iter()
                .enumerate()
                .map(|(at, entry)| (entry.id, at as u32)),
        );
    }
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
    /// [`Self::pre_record`], left alone while the pass is in step, and set
    /// from its entries when it leaves. Independent of the `(layer, node)`
    /// of the actual record, so `Widget::resolve` answers the right id
    /// before any node exists.
    counters: WidgetIdMap<u32>,
    /// Every id this pass handed out, with the endpoint of each one that
    /// has opened. [`Self::resolve`] reserves, and
    /// [`Self::record_endpoint`] (from `Forest::open_node`) records.
    /// Read for explicit-collision endpoint resolution (the first
    /// endpoint lives under `raw_id`, which is the un-disambiguated form
    /// of any subsequent occurrence). The recorded entries feed the
    /// [`Self::rollover`] removed-diff and the
    /// [`crate::cascade::Cascade::by_id`] snapshot taken at the end of
    /// each `CascadeEngine::run`; a reserved one is in neither.
    curr: IdTable,
    /// How many of `curr`'s entries match `prev`'s, once the pass has left
    /// step; `None` while every entry so far matches, and `curr`'s index is
    /// still unbuilt.
    split: Option<usize>,
    /// Last *painted* frame's `curr`. Only which ids were recorded
    /// matters for the rollover diff — endpoints are stale across
    /// frames. Same type as `curr` so `std::mem::swap` is alloc-free.
    ///
    /// [`Cascade::by_id`](crate::cascade::Cascade) holds the same
    /// entries with live values and still cannot serve this diff: it is
    /// refreshed at every cascade *run*, so a two-pass frame overwrites
    /// it before rollover and the diff would compare pass A against
    /// pass B rather than frame against frame.
    prev: IdTable,
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
        let matched = self.split.unwrap_or(self.curr.entries.len());
        let prev = &self.prev;
        // The matching prefix holds `prev`'s own ids at their positions, so
        // it is read by position, and only the rest is probed.
        let (stepped, rest) = self.curr.entries.split_at(matched);
        let new_in_step = stepped.iter().zip(&prev.entries).filter(|(entry, last)| {
            entry.slot != IdSlot::Reserved && last.slot == IdSlot::Reserved
        });
        let new_past_step = rest
            .iter()
            .filter(|entry| entry.slot != IdSlot::Reserved && !prev.recorded(entry.id));
        self.discarded.extend(
            new_in_step
                .map(|(entry, _)| entry.id)
                .chain(new_past_step.map(|entry| entry.id)),
        );
        self.curr.clear();
        self.split = None;
        self.pending.clear();
    }

    /// Where `id` was recorded this pass, or `None` when it was not —
    /// absent, or only reserved.
    #[inline]
    pub(crate) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        if self.split.is_some() {
            return self.curr.slot(id).and_then(IdSlot::endpoint);
        }
        // In step: `curr` is a prefix of `prev`, so `prev`'s index finds
        // the position.
        let at = *self.prev.index.get(&id)? as usize;
        let entry = self.curr.entries.get(at)?;
        debug_assert_eq!(entry.id, id, "an in-step entry left its position");
        entry.slot.endpoint()
    }

    /// The ids recorded this pass, with their endpoints.
    #[inline]
    pub(crate) fn recorded(&self) -> impl Iterator<Item = (WidgetId, Endpoint)> + '_ {
        self.curr.recorded_entries()
    }

    /// Push the reserved entry for `id`, which `origin` resolved to. Out
    /// of step, the caller has just put its index in the hash index.
    #[inline]
    fn reserve(&mut self, id: WidgetId, origin: Origin) -> ResolvedId {
        let entry = self.curr.entries.len() as u32;
        self.curr.entries.push(IdEntry {
            id,
            slot: IdSlot::Reserved,
            origin,
        });
        ResolvedId { id, entry }
    }

    /// Index the entries so far, and set the counters from them: the
    /// probing resolve the pass continues with reads both. The counters
    /// only let a collision skip the occurrences already taken, so the
    /// in-step resolves leave them alone and this pays one insert per
    /// colliding raw id, once.
    #[cold]
    fn leave_step(&mut self) {
        self.split = Some(self.curr.entries.len());
        self.curr.index_entries();
        for entry in &self.curr.entries {
            if entry.origin.occurrence != 0 {
                self.counters
                    .insert(entry.origin.raw, entry.origin.occurrence);
            }
        }
    }

    /// What `raw` resolves to while the pass is in step, when no probe
    /// this pass could answer differently; `None` leaves step.
    ///
    /// - An explicit id held by a reservation of this pass is its owner
    ///   claiming it, and pushes no entry. The newest entry is the usual
    ///   holder — a widget resolves, then records a wrapper under the id
    ///   it got — and any other is found through `prev`'s index, since
    ///   this pass holds `prev`'s ids at their positions.
    /// - Otherwise the id is last frame's next entry, when that entry came
    ///   from the same raw id and kind, and was no explicit collision:
    ///   whether one of those claims a reservation depends on record
    ///   order.
    #[inline]
    fn in_step(
        &mut self,
        raw: WidgetId,
        is_explicit: bool,
        recipe: Option<Recipe>,
    ) -> Option<ResolvedId> {
        if self.split.is_some() {
            return None;
        }
        let at = self.curr.entries.len();
        if is_explicit
            && self
                .curr
                .entries
                .last()
                .is_some_and(|newest| newest.id == raw)
        {
            return self.claim(at - 1);
        }
        if let Some(last) = self.prev.entries.get(at)
            && last.origin.raw == raw
            && last.origin.is_explicit == is_explicit
            && (!is_explicit || last.origin.occurrence == 0)
        {
            let id = last.id;
            let occurrence = last.origin.occurrence;
            return Some(self.reserve(
                id,
                Origin {
                    raw,
                    is_explicit,
                    occurrence,
                    recipe,
                },
            ));
        }
        if !is_explicit {
            return None;
        }
        let holder = *self.prev.index.get(&raw)? as usize;
        if holder >= at {
            return None;
        }
        self.claim(holder)
    }

    /// The claim of entry `holder` by its owner's explicit resolve, while
    /// it is only reserved; `None` once it is recorded, which makes the
    /// resolve an explicit collision.
    #[inline]
    fn claim(&self, holder: usize) -> Option<ResolvedId> {
        let entry = &self.curr.entries[holder];
        (entry.slot == IdSlot::Reserved).then_some(ResolvedId {
            id: entry.id,
            entry: holder as u32,
        })
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
    pub(crate) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> ResolvedId {
        self.resolve_raw(raw_id, is_explicit, None)
    }

    /// [`Self::resolve`] for a parent-scoped ident — an auto id's call site
    /// or a salt — under `parent`. In step, last frame's entry at this
    /// position names the same raw id when it came from the same inputs,
    /// so the id is taken from it and the raw id is never hashed: an auto
    /// id's file path, or a salt's mix with the parent. An explicit salt
    /// takes it only when last frame's did not collide, and then it is no
    /// claim either, since that id is not yet taken. Otherwise the raw id
    /// is hashed, by [`Ident::raw_id`] as for any other ident.
    #[inline]
    pub(crate) fn resolve_scoped(&mut self, ident: Ident, parent: Option<WidgetId>) -> ResolvedId {
        debug_assert!(matches!(ident, Ident::Auto(_) | Ident::Hash(_)));
        let recipe = Recipe { ident, parent };
        let is_explicit = ident.is_explicit();
        if self.split.is_none()
            && let Some(last) = self.prev.entries.get(self.curr.entries.len())
            && last.origin.recipe.is_some_and(|last| last.is(recipe))
            && (!is_explicit || last.origin.occurrence == 0)
        {
            let (id, origin) = (last.id, last.origin);
            return self.reserve(
                id,
                Origin {
                    recipe: Some(recipe),
                    ..origin
                },
            );
        }
        self.resolve_raw(ident.raw_id(parent), is_explicit, Some(recipe))
    }

    /// [`Self::resolve`], recording the `recipe` of a parent-scoped ident.
    #[inline]
    fn resolve_raw(
        &mut self,
        raw_id: WidgetId,
        is_explicit: bool,
        recipe: Option<Recipe>,
    ) -> ResolvedId {
        if let Some(resolved) = self.in_step(raw_id, is_explicit, recipe) {
            return resolved;
        }
        if self.split.is_none() {
            self.leave_step();
        }
        let next = self.curr.entries.len() as u32;
        match self.curr.index.entry(raw_id) {
            // Fast path — first occurrence. `counters` only tracks raw
            // ids that actually collided, so its size is
            // `collisions / frame` (typically 0), not `widgets / frame`.
            Entry::Vacant(slot) => {
                slot.insert(next);
                return self.reserve(
                    raw_id,
                    Origin {
                        raw: raw_id,
                        is_explicit,
                        occurrence: 0,
                        recipe,
                    },
                );
            }
            // An explicit id that is only reserved is the widget that
            // reserved it, claiming it: a widget resolves its own id,
            // then records a wrapper under `.id(resolved)`. Only an auto
            // id, which two call-site twins can share, disambiguates
            // against a reservation.
            Entry::Occupied(slot)
                if is_explicit
                    && self.curr.entries[*slot.get() as usize].slot == IdSlot::Reserved =>
            {
                return ResolvedId {
                    id: raw_id,
                    entry: *slot.get(),
                };
            }
            Entry::Occupied(_) => {}
        }
        let mut count = self.counters.get(&raw_id).copied().unwrap_or(0);
        let final_id = loop {
            count = count
                .checked_add(1)
                .expect("WidgetId occurrence counter overflowed");
            let candidate = raw_id.with(count);
            if let Entry::Vacant(slot) = self.curr.index.entry(candidate) {
                slot.insert(next);
                break candidate;
            }
        };
        self.counters.insert(raw_id, count);
        if is_explicit {
            self.pending.push(PendingExplicitCollision {
                first_raw_id: raw_id,
                second_final_id: final_id,
            });
        }
        self.reserve(
            final_id,
            Origin {
                raw: raw_id,
                is_explicit,
                occurrence: count,
                recipe,
            },
        )
    }

    /// Record the endpoint where `resolved` is being opened. `Some`
    /// when this endpoint completed a [`PendingExplicitCollision`]
    /// queued at [`Self::resolve`], pairing it with the first
    /// occurrence's endpoint — `None` on every other open, which is the
    /// common case for every node of every frame.
    ///
    /// Panics if `resolved` was already recorded this pass, or was
    /// resolved in an earlier one. [`Self::resolve`] must return an
    /// available id, and the check enforces that invariant without
    /// overwriting the existing endpoint.
    #[inline]
    pub(crate) fn record_endpoint(
        &mut self,
        resolved: ResolvedId,
        endpoint: Endpoint,
    ) -> Option<CollisionRecord> {
        let final_id = resolved.id();
        let entry = self
            .curr
            .entries
            .get_mut(resolved.entry as usize)
            .filter(|entry| entry.id == final_id)
            .unwrap_or_else(|| stale_resolved_id(final_id));
        assert!(
            entry.slot == IdSlot::Reserved,
            "record_endpoint called twice for {final_id:?}"
        );
        entry.slot = IdSlot::Recorded(endpoint);
        // Scanned rather than mapped: an explicit collision is a caller
        // bug, so `pending` is empty on the frames that matter and this
        // is a length test — where a hash probe would cost every node of
        // every frame.
        //
        // Either side may record last: a widget that resolved before
        // recording lets its duplicate open first. The pair completes
        // on whichever record supplies the second endpoint.
        let (idx, record) = self.pending.iter().enumerate().find_map(|(idx, p)| {
            if p.second_final_id != final_id && p.first_raw_id != final_id {
                return None;
            }
            Some((
                idx,
                CollisionRecord {
                    first: self.endpoint(p.first_raw_id)?,
                    second: self.endpoint(p.second_final_id)?,
                },
            ))
        })?;
        self.pending.swap_remove(idx);
        Some(record)
    }

    /// Populate `self.removed` with widgets present in `prev` but
    /// absent from `curr`, then swap `curr → prev` so the next frame
    /// diffs against this one. Returns a borrow of `self.removed`
    /// for callers that want to fan the diff straight into per-widget
    /// caches (text shaper, measure cache, state map, animation,
    /// damage); the field stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &WidgetIdSet {
        self.removed.clear();
        let matched = self.split.unwrap_or(self.curr.entries.len());
        for (at, last) in self.prev.entries.iter().enumerate() {
            if last.slot == IdSlot::Reserved {
                continue;
            }
            // Past the matching prefix, an in-step `curr` holds none of
            // `prev`'s ids: its own are the prefix's, and they are distinct.
            let kept = match self.curr.entries.get(at) {
                Some(entry) if at < matched => entry.slot != IdSlot::Reserved,
                _ => self.split.is_some() && self.curr.recorded(last.id),
            };
            if !kept {
                self.removed.insert(last.id);
            }
        }
        // Ids seen only in a discarded pass this frame (double-layout
        // pass A, cold-start warmup) are in neither `prev` nor `curr`
        // — the prev-minus-curr diff can't see them, but any state /
        // anim / measure / text rows they created during that pass are
        // real and must be swept with everything else.
        for &wid in &self.discarded {
            if self.endpoint(wid).is_none() {
                self.removed.insert(wid);
            }
        }
        self.discarded.clear();
        if self.split.is_some() {
            mem::swap(&mut self.curr, &mut self.prev);
            self.curr.clear();
        } else {
            // In step, `curr` is a prefix of `prev`, so `prev`'s index
            // already holds every position it needs once the tail leaves.
            // It only ever loses ids here, so it never rehashes.
            for last in &self.prev.entries[matched..] {
                self.prev.index.remove(&last.id);
            }
            mem::swap(&mut self.curr.entries, &mut self.prev.entries);
            self.curr.entries.clear();
        }
        self.split = None;
        &self.removed
    }
}

/// Outlined from [`SeenIds::record_endpoint`], which runs for every node:
/// the message is for a caller bug, not for the hot path.
#[cold]
#[inline(never)]
fn stale_resolved_id(id: WidgetId) -> ! {
    panic!("record_endpoint given {id:?}, which was not resolved this pass")
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetIdMap;
    use crate::scene::endpoint::Endpoint;
    use crate::scene::seen_ids::SeenIds;

    impl SeenIds {
        /// The ids the last finished frame recorded, with their endpoints.
        /// Valid between frames: `rollover` ends a frame by moving `curr`
        /// to `prev`.
        pub(crate) fn last_frame(&self) -> WidgetIdMap<Endpoint> {
            self.prev.recorded_entries().collect()
        }
    }
}

#[cfg(test)]
mod tests;
