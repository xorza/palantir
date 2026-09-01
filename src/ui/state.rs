//! Cross-frame widget state. Per-T dense `Vec<T>` stores indexed by
//! `WidgetId`, held in a [`TypedStores`] — one boxed store per
//! *distinct T* (typically a handful), not per widget. Steady-state allocation is zero after
//! warmup — `Vec<T>` capacity is reused across frames, no per-row
//! `Box`, no `Any` downcast on the hot path.
//!
//! Reusing a `WidgetId` with two different `T`s is a caller bug — the
//! two rows live in different stores and don't see each other. Not
//! checked; debug aid wasn't worth a hashmap probe per call.
//!
//! Sweep: when a widget stops being recorded, `FrameCycle::finalize_frame`
//! calls `sweep_removed` with the diff (once per frame, after the
//! final record pass); each per-T store `swap_remove`s affected rows
//! and patches the swapped neighbour's index in O(1) using the
//! parallel `owners` vec.

use crate::common::typed_stores::{Drained, TypedStore, TypedStores};
use crate::primitives::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};

#[derive(Debug, Default)]
pub(crate) struct StateMap {
    stores: TypedStores,
}

impl StateMap {
    pub(super) fn get_or_insert_with<T, F>(&mut self, id: WidgetId, init: F) -> &mut T
    where
        T: 'static,
        F: FnOnce() -> T,
    {
        self.stores
            .get_or_default::<Store<T>>()
            .get_or_insert_with(id, init)
    }

    pub(super) fn try_get<T: 'static>(&self, id: WidgetId) -> Option<&T> {
        self.stores.get::<Store<T>>()?.try_get(id)
    }

    pub(super) fn try_get_mut<T: 'static>(&mut self, id: WidgetId) -> Option<&mut T> {
        self.stores.get_mut::<Store<T>>()?.try_get_mut(id)
    }

    /// Caller guards on `removed` being non-empty — see
    /// `FrameCycle::finalize_frame`, which shares that one guard with the
    /// other removal-driven sweep.
    pub(super) fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        self.stores.sweep_removed(removed, Drained::Keep);
    }
}

#[derive(Debug)]
struct Store<T> {
    map: WidgetIdMap<u32>,
    data: Vec<T>,
    owners: Vec<WidgetId>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            map: WidgetIdMap::default(),
            data: Vec::new(),
            owners: Vec::new(),
        }
    }
}

impl<T> Store<T> {
    /// The row `id` occupies, if it has one. The map column is `u32` to
    /// stay narrow; every reader indexes with a `usize`.
    fn index_of(&self, id: WidgetId) -> Option<usize> {
        self.map.get(&id).map(|&idx| idx as usize)
    }

    fn try_get(&self, id: WidgetId) -> Option<&T> {
        Some(&self.data[self.index_of(id)?])
    }

    fn try_get_mut(&mut self, id: WidgetId) -> Option<&mut T> {
        let idx = self.index_of(id)?;
        Some(&mut self.data[idx])
    }

    fn get_or_insert_with<F: FnOnce() -> T>(&mut self, id: WidgetId, init: F) -> &mut T {
        let idx = match self.index_of(id) {
            Some(idx) => idx,
            None => {
                let idx = self.data.len();
                debug_assert!(idx < u32::MAX as usize, "StateMap store overflow");
                self.data.push(init());
                self.owners.push(id);
                self.map.insert(id, idx as u32);
                idx
            }
        };
        &mut self.data[idx]
    }
}

impl<T: 'static> TypedStore for Store<T> {
    /// `swap_remove` the row, then patch the swapped neighbour's index
    /// in O(1) off the parallel `owners` vec.
    fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        for id in removed {
            let Some(idx) = self.map.remove(id) else {
                continue;
            };
            let idx = idx as usize;
            let last = self.data.len() - 1;
            self.data.swap_remove(idx);
            self.owners.swap_remove(idx);
            if idx != last {
                let moved = self.owners[idx];
                self.map.insert(moved, idx as u32);
            }
        }
    }

    /// Only read by the drop-drained sweep, which state does not use —
    /// an empty per-`T` store costs one hashmap slot and is reused the
    /// next time a widget of that type appears.
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::state::*;

    fn wid(n: u64) -> WidgetId {
        WidgetId::from_hash(n)
    }

    #[test]
    fn value_persists_across_frames() {
        let mut map = StateMap::default();
        assert!(map.try_get_mut::<u32>(wid(1)).is_none());
        assert!(
            map.stores.is_empty(),
            "missing mutable probe must not create a typed store",
        );
        *map.get_or_insert_with(wid(1), || 0u32) = 42;
        *map.try_get_mut::<u32>(wid(1)).unwrap() = 43;
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 43);
        assert!(map.try_get_mut::<u32>(wid(2)).is_none());
    }

    #[test]
    fn init_only_runs_on_first_insert() {
        let mut map = StateMap::default();
        let mut init_calls = 0u32;
        for _ in 0..3 {
            let _ = map.get_or_insert_with(wid(1), || {
                init_calls += 1;
                7u32
            });
        }
        assert_eq!(init_calls, 1);
    }

    #[test]
    fn distinct_ids_in_same_store_dont_alias() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 11;
        *map.get_or_insert_with(wid(2), || 0u32) = 22;
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 11);
        assert_eq!(*map.get_or_insert_with(wid(2), || 0u32), 22);
    }

    #[test]
    fn distinct_types_at_distinct_ids_coexist() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 11;
        *map.get_or_insert_with(wid(2), String::new) = "hi".into();
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 11);
        assert_eq!(map.get_or_insert_with(wid(2), String::new), "hi");
    }

    #[test]
    fn sweep_removed_drops_rows() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 99;
        map.sweep_removed(&WidgetIdSet::from_iter([wid(1)]));
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 0);
    }

    #[test]
    fn sweep_patches_swapped_index() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 1;
        *map.get_or_insert_with(wid(2), || 0u32) = 2;
        *map.get_or_insert_with(wid(3), || 0u32) = 3;
        // Drop the middle one; `wid(3)` was at idx 2, must end at idx 1
        // and still read back as 3.
        map.sweep_removed(&WidgetIdSet::from_iter([wid(2)]));
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 1);
        assert_eq!(*map.get_or_insert_with(wid(3), || 0u32), 3);
    }
}
