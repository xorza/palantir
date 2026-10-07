//! Cross-frame widget state: dense `Vec<T>` stores indexed by `WidgetId` in a [`TypedStores`], one boxed store per distinct `T`. Allocation-free after warmup; no `Any` downcast on the hot path.
//!
//! Reusing a `WidgetId` with two `T`s is a caller bug: the rows live in separate stores. Unchecked, to avoid a probe per call.
//!
//! When a widget stops being recorded, `FrameCycle::finalize_frame` calls `sweep_removed` once per frame; each store `swap_remove`s the row and patches the swapped neighbour via the `owners` vec in O(1).

use crate::common::typed_stores::{Drained, TypedStore, TypedStores};
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};

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

    /// Drop the rows of the widgets in `removed`, one probe each.
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
    /// The row `id` occupies, if any. The map column is `u32` to stay narrow; readers index with `usize`.
    fn index_of(&self, id: WidgetId) -> Option<usize> {
        self.map.get(&id).map(|&idx| idx as usize)
    }

    fn try_get(&self, id: WidgetId) -> Option<&T> {
        Some(&self.data[self.index_of(id)?])
    }

    fn get_or_insert_with<F: FnOnce() -> T>(&mut self, id: WidgetId, init: F) -> &mut T {
        let idx = if let Some(idx) = self.index_of(id) {
            idx
        } else {
            let idx = self.data.len();
            debug_assert!(idx < u32::MAX as usize, "StateMap store overflow");
            self.data.push(init());
            self.owners.push(id);
            self.map.insert(id, idx as u32);
            idx
        };
        &mut self.data[idx]
    }
}

impl<T: 'static> TypedStore for Store<T> {
    /// `swap_remove` the row, then patch the swapped neighbour's index off `owners`.
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

    /// Read only by the drop-drained sweep, which state doesn't use; an empty per-`T` store costs one hashmap slot and is reused.
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
        assert!(map.try_get::<u32>(wid(1)).is_none());
        assert!(
            map.stores.is_empty(),
            "a missing probe must not create a typed store",
        );
        *map.get_or_insert_with(wid(1), || 0u32) = 42;
        *map.get_or_insert_with(wid(1), || 0u32) = 43;
        assert_eq!(map.try_get::<u32>(wid(1)), Some(&43));
        assert!(map.try_get::<u32>(wid(2)).is_none());
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
        // Drop the middle one: `wid(3)` moves from idx 2 to 1 and still reads back as 3.
        map.sweep_removed(&WidgetIdSet::from_iter([wid(2)]));
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 1);
        assert_eq!(*map.get_or_insert_with(wid(3), || 0u32), 3);
    }
}
