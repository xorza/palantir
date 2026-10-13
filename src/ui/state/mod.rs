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
        #[expect(
            clippy::iter_over_hash_type,
            reason = "a fixed hasher, so the rows end in the same slots from run to run"
        )]
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
mod tests;
