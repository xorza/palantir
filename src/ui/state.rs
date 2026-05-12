//! Cross-frame widget state. Per-T dense `Vec<T>` stores indexed by
//! `WidgetId`; one `Box<dyn AnyTyped>` per *distinct T* (typically a
//! handful), not per widget. Steady-state allocation is zero after
//! warmup — `Vec<T>` capacity is reused across frames, no per-row
//! `Box`, no `Any` downcast on the hot path.
//!
//! Reusing a `WidgetId` with two different `T`s is a caller bug — the
//! two rows live in different stores and don't see each other. Not
//! checked; debug aid wasn't worth a hashmap probe per call.
//!
//! Sweep: when a widget stops being recorded, `Ui::post_record` calls
//! `sweep_removed` with the diff; each per-T store `swap_remove`s
//! affected rows and patches the swapped neighbour's index in O(1)
//! using the parallel `owners` vec.

use crate::forest::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::any::{Any, TypeId};

#[derive(Default)]
pub(crate) struct StateMap {
    by_type: FxHashMap<TypeId, Box<dyn AnyTyped>>,
}

impl StateMap {
    pub(crate) fn get_or_insert_with<T, F>(&mut self, id: WidgetId, init: F) -> &mut T
    where
        T: 'static,
        F: FnOnce() -> T,
    {
        self.typed_mut::<T>().get_or_insert_with(id, init)
    }

    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        if removed.is_empty() {
            return;
        }
        for typed in self.by_type.values_mut() {
            typed.sweep_removed(removed);
        }
    }

    fn typed_mut<T: 'static>(&mut self) -> &mut Store<T> {
        self.by_type
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::<Store<T>>::default())
            .as_any_mut()
            .downcast_mut::<Store<T>>()
            .expect("TypeId is stable per T, downcast cannot fail")
    }
}

struct Store<T> {
    map: FxHashMap<WidgetId, u32>,
    data: Vec<T>,
    owners: Vec<WidgetId>,
}

impl<T> Default for Store<T> {
    fn default() -> Self {
        Self {
            map: FxHashMap::default(),
            data: Vec::new(),
            owners: Vec::new(),
        }
    }
}

impl<T> Store<T> {
    fn get_or_insert_with<F: FnOnce() -> T>(&mut self, id: WidgetId, init: F) -> &mut T {
        let idx = match self.map.get(&id) {
            Some(&idx) => idx as usize,
            None => {
                let idx = self.data.len();
                assert!(idx < u32::MAX as usize, "StateMap store overflow");
                self.data.push(init());
                self.owners.push(id);
                self.map.insert(id, idx as u32);
                idx
            }
        };
        &mut self.data[idx]
    }

    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
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
}

trait AnyTyped: Any {
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> AnyTyped for Store<T> {
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        Store::<T>::sweep_removed(self, removed);
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wid(n: u64) -> WidgetId {
        WidgetId::from_hash(n)
    }

    #[test]
    fn value_persists_across_frames() {
        let mut map = StateMap::default();
        *map.get_or_insert_with(wid(1), || 0u32) = 42;
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 42);
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
        map.sweep_removed(&FxHashSet::from_iter([wid(1)]));
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
        map.sweep_removed(&FxHashSet::from_iter([wid(2)]));
        assert_eq!(*map.get_or_insert_with(wid(1), || 0u32), 1);
        assert_eq!(*map.get_or_insert_with(wid(3), || 0u32), 3);
    }
}
