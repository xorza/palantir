//! Per-frame `WidgetId` tracker for "which widgets were recorded this frame":
//!
//! 1. **Eager disambiguation.** [`SeenIds::resolve`] runs at `Widget::resolve`
//!    time, before `record`, mixing an occurrence counter into a raw id already
//!    handed out, so the id matches what the tree, cascade and `response_for` see.
//!    Explicit-key collisions (`.id(X)`, `.id_salt(X)`) are caller bugs: `resolve`
//!    queues a [`PendingExplicitCollision`] and [`SeenIds::record_endpoint`]
//!    finalizes the [`CollisionRecord`] once both opens report. An id is taken from
//!    the moment `resolve` hands it out ([`IdSlot::Reserved`]), so widgets that all
//!    resolve before recording still get distinct ids. The [`ResolvedId`] carries
//!    the entry's index, saving a second probe.
//! 2. **Endpoint tracking.** [`SeenIds::record_endpoint`] (at `Forest::open_node`)
//!    turns the slot into [`IdSlot::Recorded`].
//! 3. **Removed-widget diff and rollover.** [`SeenIds::rollover`] puts ids present
//!    in the last painted frame but absent now into `removed` (for damage, text,
//!    measure cache, state, animation), then swaps `curr` and `prev`, once per
//!    frame from `FrameCycle::finalize_frame`. Ids seen only in a discarded pass go
//!    to `discarded` at the next `pre_record` and fold into `removed`, or their rows
//!    would leak and resume stale.
//!
//! **In step with the last frame.** A frame usually resolves the same raw ids in
//! the same order. While it does, each id is known without a probe (last frame's
//! ids are distinct and this pass holds only earlier ones), so `curr` builds no
//! hash index until the first differing resolve, then indexes the entries so far
//! and sets the counters. The rollover diff compares by position over the matching
//! prefix. A steady frame hashes no id.

use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::scene::endpoint::Endpoint;
use crate::scene::node::ident::Ident;
use std::collections::hash_map::Entry;
use std::mem;
use std::ptr;

/// Both nodes of one explicit-id collision, returned by
/// [`SeenIds::record_endpoint`] when an endpoint completes a pair.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CollisionRecord {
    pub(crate) first: Endpoint,
    pub(crate) second: Endpoint,
}

/// What one id holds in [`SeenIds::curr`] this pass. One table, so resolve pays
/// one probe and record none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdSlot {
    /// Handed out by [`SeenIds::resolve`], not yet recorded. Another widget may
    /// resolve the same raw id in between (`.state(ui)` on two same-site buttons, then
    /// `.show()`), so it counts as taken. Lasts one pass.
    Reserved,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LastFrame {
    At(Endpoint),
    Absent,
    Unknown,
}

/// An id [`SeenIds::resolve`] handed out this pass, with its entry's index.
/// [`SeenIds::record_endpoint`] still checks the id against it: a stale value from
/// an earlier pass would write another widget's entry. Packed to 4-byte alignment
/// to fit beside [`Ident`](crate::scene::node::ident::Ident)'s tag.
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct IdEntry {
    id: WidgetId,
    slot: IdSlot,
    origin: Origin,
}

/// How a pass came to an entry's id: what the next frame's resolve at the same
/// position must repeat to stay in step.
#[derive(Clone, Copy, Debug)]
struct Origin {
    raw: WidgetId,
    is_explicit: bool,
    occurrence: u32,
    recipe: Option<Recipe>,
}

#[derive(Clone, Copy, Debug)]
struct Recipe {
    ident: Ident,
    parent: Option<WidgetId>,
}

impl Recipe {
    /// Whether `other` names the same raw id by the same inputs. A `Location` is
    /// compared by address; a duplicated call site just fails to match.
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

/// One pass's ids: a hash index over a dense run of entries in resolve order.
/// `curr`'s index is empty while in step with `prev`.
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

    #[inline]
    fn recorded(&self, id: WidgetId) -> bool {
        self.slot(id).and_then(IdSlot::endpoint).is_some()
    }

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

/// One side of a queued explicit-collision pair; the second endpoint is filled in
/// at [`SeenIds::record_endpoint`].
#[derive(Clone, Copy, Debug)]
struct PendingExplicitCollision {
    first_raw_id: WidgetId,
    second_final_id: WidgetId,
}

#[derive(Debug, Default)]
pub(crate) struct SeenIds {
    /// Per-raw-id occurrence counter, bumped in [`Self::resolve`] on collision. Cleared
    /// in [`Self::pre_record`], untouched while in step, set from the entries when the
    /// pass leaves step.
    counters: WidgetIdMap<u32>,
    /// Every id this pass handed out, with the endpoint of each that has opened.
    /// Reserved entries feed neither the [`Self::rollover`] diff nor the
    /// [`crate::cascade::Cascade::by_id`] snapshot.
    curr: IdTable,
    /// How many of `curr`'s entries match `prev`'s once the pass leaves step; `None`
    /// while all match.
    split: Option<usize>,
    /// Last painted frame's `curr`; same type so the swap is alloc-free.
    /// [`Cascade::by_id`](crate::cascade::Cascade) can't serve this diff: it refreshes
    /// every cascade run, so a two-pass frame would compare pass A with pass B.
    prev: IdTable,
    /// Widgets in `prev` but not `curr`. A public field so callers can hold
    /// `&seen.removed` across other `&forest` reads.
    pub(crate) removed: WidgetIdSet,
    /// Explicit collisions awaiting [`Self::record_endpoint`]. Cleared each frame.
    pending: Vec<PendingExplicitCollision>,
    /// Ids existing only in this frame's discarded passes and absent from `prev`.
    /// Folded into `removed` at [`Self::rollover`] unless the final pass re-recorded
    /// them. Ids `prev` holds stay out (the diff reports them), keeping this empty on
    /// a settling second pass.
    discarded: WidgetIdSet,
}

impl SeenIds {
    /// Reset per-frame state at the top of a record pass. `prev` is untouched, since a
    /// discarded pass never reaches `rollover`. A non-empty `curr` here is a discarded
    /// pass: ids `prev` has never seen move to `discarded`.
    pub(crate) fn pre_record(&mut self) {
        self.counters.clear();
        let matched = self.split.unwrap_or(self.curr.entries.len());
        let prev = &self.prev;
        // The matching prefix holds `prev`'s ids at their positions; only the rest is probed.
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

    #[inline]
    pub(crate) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        if self.split.is_some() {
            return self.curr.slot(id).and_then(IdSlot::endpoint);
        }
        let at = *self.prev.index.get(&id)? as usize;
        let entry = self.curr.entries.get(at)?;
        debug_assert_eq!(entry.id, id, "an in-step entry left its position");
        entry.slot.endpoint()
    }

    /// Where the last painted frame recorded `id`, when known without a probe (the
    /// newest entry of a pass in step, the usual case). Otherwise [`LastFrame::Unknown`].
    #[inline]
    pub(crate) fn last_frame_endpoint(&self, id: WidgetId) -> LastFrame {
        let newest = self.curr.entries.len().wrapping_sub(1);
        if self.split.is_some()
            || self
                .curr
                .entries
                .get(newest)
                .is_none_or(|entry| entry.id != id)
        {
            return LastFrame::Unknown;
        }
        match self.prev.entries[newest].slot.endpoint() {
            Some(endpoint) => LastFrame::At(endpoint),
            None => LastFrame::Absent,
        }
    }

    #[inline]
    pub(crate) fn recorded(&self) -> impl Iterator<Item = (WidgetId, Endpoint)> + '_ {
        self.curr.recorded_entries()
    }

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

    /// Index the entries so far and set the counters, for the probing resolve the pass
    /// continues with. Pays one insert per colliding raw id, once.
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

    /// What `raw` resolves to while in step, when no probe could differ; `None` leaves
    /// step. An explicit id held by a reservation of this pass is its owner claiming it
    /// and pushes no entry. Otherwise the id is last frame's next entry, if it came
    /// from the same raw id and kind and was no explicit collision.
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

    #[inline]
    fn claim(&self, holder: usize) -> Option<ResolvedId> {
        let entry = &self.curr.entries[holder];
        (entry.slot == IdSlot::Reserved).then_some(ResolvedId {
            id: entry.id,
            entry: holder as u32,
        })
    }

    /// Eagerly resolve a raw id to its final id and reserve it until
    /// [`Self::record_endpoint`]. A collision advances the counter until
    /// `raw_id.with(count)` is neither recorded nor reserved. Explicit collisions queue
    /// a [`PendingExplicitCollision`], whichever order the two record.
    #[inline]
    pub(crate) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> ResolvedId {
        self.resolve_raw(raw_id, is_explicit, None)
    }

    /// [`Self::resolve`] for a parent-scoped ident. In step, last frame's entry at this
    /// position supplies the id when it came from the same inputs, so the raw id is
    /// never hashed. An explicit salt takes it only when last frame's did not collide.
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
            // First occurrence. `counters` tracks only collided raw ids.
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
            // An explicit id that is only reserved is its owner claiming it (resolve, then a
            // wrapper under `.id(resolved)`). Only an auto id, which call-site twins share,
            // disambiguates against a reservation.
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

    /// Record the endpoint where `resolved` opens. `Some` when it completed a
    /// [`PendingExplicitCollision`]; `None` otherwise, the common case. Panics if
    /// `resolved` was already recorded this pass or resolved in an earlier one.
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
        // Scanned, not mapped: explicit collisions are caller bugs, so `pending` is
        // usually empty and a probe would cost every node. Either side may record last.
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

    /// Populate `self.removed` with widgets in `prev` but not `curr`, then swap. Returns
    /// a borrow of `self.removed`, which stays populated until the next `rollover`.
    pub(crate) fn rollover(&mut self) -> &WidgetIdSet {
        self.removed.clear();
        let matched = self.split.unwrap_or(self.curr.entries.len());
        for (at, last) in self.prev.entries.iter().enumerate() {
            if last.slot == IdSlot::Reserved {
                continue;
            }
            // Past the matching prefix an in-step `curr` holds none of `prev`'s ids.
            let kept = match self.curr.entries.get(at) {
                Some(entry) if at < matched => entry.slot != IdSlot::Reserved,
                _ => self.split.is_some() && self.curr.recorded(last.id),
            };
            if !kept {
                self.removed.insert(last.id);
            }
        }
        // Ids seen only in a discarded pass are in neither `prev` nor `curr`, but the rows
        // they created must be swept.
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
            // In step `curr` is a prefix of `prev`, so `prev`'s index already holds every
            // position needed; it only loses ids, so never rehashes.
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

/// Outlined from [`SeenIds::record_endpoint`], which runs per node.
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
        pub(crate) fn last_frame(&self) -> WidgetIdMap<Endpoint> {
            self.prev.recorded_entries().collect()
        }
    }
}

#[cfg(test)]
mod tests;
