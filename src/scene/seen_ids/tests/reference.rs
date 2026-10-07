//! The id tracker before its in-step path: one hash table per pass, cleared and refilled every frame; the differential test holds [`SeenIds`](crate::scene::seen_ids::SeenIds) to it.

use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};
use crate::scene::endpoint::Endpoint;
use crate::scene::seen_ids::{CollisionRecord, ResolvedId};
use std::mem;

/// An id one pass handed out and, once recorded, where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    id: WidgetId,
    endpoint: Option<Endpoint>,
}

#[derive(Debug, Default)]
struct Table {
    index: WidgetIdMap<usize>,
    entries: Vec<Entry>,
}

impl Table {
    fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.entries[*self.index.get(&id)?].endpoint
    }

    fn recorded(&self) -> impl Iterator<Item = (WidgetId, Endpoint)> + '_ {
        self.entries
            .iter()
            .filter_map(|entry| Some((entry.id, entry.endpoint?)))
    }

    fn clear(&mut self) {
        self.index.clear();
        self.entries.clear();
    }
}

#[derive(Debug, Default)]
pub(super) struct Reference {
    counters: WidgetIdMap<u32>,
    curr: Table,
    prev: Table,
    pending: Vec<(WidgetId, WidgetId)>,
    discarded: WidgetIdSet,
}

impl Reference {
    pub(super) fn pre_record(&mut self) {
        self.counters.clear();
        let prev = &self.prev;
        self.discarded.extend(
            self.curr
                .recorded()
                .map(|(id, _)| id)
                .filter(|&id| prev.endpoint(id).is_none()),
        );
        self.curr.clear();
        self.pending.clear();
    }

    pub(super) fn endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.curr.endpoint(id)
    }

    pub(super) fn last_frame_endpoint(&self, id: WidgetId) -> Option<Endpoint> {
        self.prev.endpoint(id)
    }

    pub(super) fn recorded(&self) -> Vec<(WidgetId, Endpoint)> {
        self.curr.recorded().collect()
    }

    pub(super) fn resolve(&mut self, raw_id: WidgetId, is_explicit: bool) -> ResolvedId {
        if let Some(&entry) = self.curr.index.get(&raw_id) {
            if is_explicit && self.curr.entries[entry].endpoint.is_none() {
                return ResolvedId {
                    id: raw_id,
                    entry: entry as u32,
                };
            }
        } else {
            return self.reserve(raw_id);
        }
        let mut count = self.counters.get(&raw_id).copied().unwrap_or(0);
        let final_id = loop {
            count += 1;
            let candidate = raw_id.with(count);
            if !self.curr.index.contains_key(&candidate) {
                break candidate;
            }
        };
        self.counters.insert(raw_id, count);
        if is_explicit {
            self.pending.push((raw_id, final_id));
        }
        self.reserve(final_id)
    }

    fn reserve(&mut self, id: WidgetId) -> ResolvedId {
        let entry = self.curr.entries.len();
        self.curr.index.insert(id, entry);
        self.curr.entries.push(Entry { id, endpoint: None });
        ResolvedId {
            id,
            entry: entry as u32,
        }
    }

    pub(super) fn record_endpoint(
        &mut self,
        resolved: ResolvedId,
        endpoint: Endpoint,
    ) -> Option<CollisionRecord> {
        let id = resolved.id();
        let entry = &mut self.curr.entries[resolved.entry as usize];
        assert_eq!(*entry, Entry { id, endpoint: None });
        entry.endpoint = Some(endpoint);
        let (idx, record) =
            self.pending
                .iter()
                .enumerate()
                .find_map(|(idx, &(first, second))| {
                    if second != id && first != id {
                        return None;
                    }
                    Some((
                        idx,
                        CollisionRecord {
                            first: self.endpoint(first)?,
                            second: self.endpoint(second)?,
                        },
                    ))
                })?;
        self.pending.swap_remove(idx);
        Some(record)
    }

    pub(super) fn rollover(&mut self) -> WidgetIdSet {
        let mut removed = WidgetIdSet::default();
        for (id, _) in self.prev.recorded() {
            if self.curr.endpoint(id).is_none() {
                removed.insert(id);
            }
        }
        for &id in &self.discarded {
            if self.curr.endpoint(id).is_none() {
                removed.insert(id);
            }
        }
        self.discarded.clear();
        mem::swap(&mut self.curr, &mut self.prev);
        self.curr.clear();
        removed
    }
}
