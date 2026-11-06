//! Cross-frame widget state: dense `Vec<Option<T>>` stores indexed by `WidgetId` in a [`TypedStores`], one boxed store per distinct `T`. Allocation-free after warmup; no `Any` downcast on the hot path.
//!
//! A row is lent out by [`StateMap::lend`] and returned by [`StateMap::put`], so a lend leaves no value behind: no `T::default()` is built per lend, as one whose default allocates would per frame.
//!
//! A row is keyed by `(WidgetId, T)`: one id holds one row of each `T`, each in its own store, as an `EditableLabel` keeps its own row beside the `TextEditState` of the editor it records under the same id.
//!
//! When a widget stops being recorded, `FrameLoop::finalize_frame` calls `sweep_removed` once per frame; each store `swap_remove`s the row and patches the swapped neighbor via the `owners` vec in O(1).

use crate::primitives::identity::typed_stores::{Drained, TypedStore, TypedStores};
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap, WidgetIdSet};

/// What a read or a lend of a lent row panics with.
pub(crate) const LENT_ROW: &str =
    "this state row is lent to a `Ui::with_state` body: read it through the body's `&mut S`";

#[derive(Debug, Default)]
pub(crate) struct StateMap {
    stores: TypedStores<WidgetIdSet>,
}

impl StateMap {
    /// Lend `id`'s value out, leaving its row empty until [`Self::put`]; `None` for a new row,
    /// which the lend makes, so a first lend refuses a second as a later one does.
    ///
    /// # Panics
    ///
    /// While the row is lent already.
    #[track_caller]
    pub(super) fn lend<T: 'static>(&mut self, id: WidgetId) -> Option<T> {
        self.stores.get_or_default::<Store<T>>().lend(id)
    }

    /// Store `value` as `id`'s, in its row or a new one.
    pub(super) fn put<T: 'static>(&mut self, id: WidgetId, value: T) {
        self.stores.get_or_default::<Store<T>>().put(id, value);
    }

    /// Store `value` as `id`'s, in place of the value its row holds or in a new row.
    ///
    /// # Panics
    ///
    /// While the row is lent out: the lend's [`Self::put`] would overwrite `value`.
    #[track_caller]
    pub(super) fn set<T: 'static>(&mut self, id: WidgetId, value: T) {
        self.stores.get_or_default::<Store<T>>().set(id, value);
    }

    /// Drop `id`'s row of `T`, if it has one, returning its value.
    ///
    /// # Panics
    ///
    /// While the row is lent out.
    #[track_caller]
    pub(super) fn remove<T: 'static>(&mut self, id: WidgetId) -> Option<T> {
        self.stores.get_mut::<Store<T>>()?.remove(id)
    }

    /// # Panics
    ///
    /// While the row is lent out.
    #[track_caller]
    pub(super) fn try_get<T: 'static>(&self, id: WidgetId) -> Option<&T> {
        self.stores.get::<Store<T>>()?.try_get(id)
    }

    /// Drop the rows of the widgets in `removed`, one probe each.
    pub(super) fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        self.stores.sweep(removed, Drained::Keep);
    }
}

#[derive(Debug)]
struct Store<T> {
    map: WidgetIdMap<u32>,
    /// `None` while the row is lent out.
    data: Vec<Option<T>>,
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
    fn idx_of(&self, id: WidgetId) -> Option<usize> {
        self.map.get(&id).map(|&idx| idx as usize)
    }

    #[track_caller]
    fn try_get(&self, id: WidgetId) -> Option<&T> {
        Some(self.data[self.idx_of(id)?].as_ref().expect(LENT_ROW))
    }

    #[track_caller]
    fn lend(&mut self, id: WidgetId) -> Option<T> {
        if let Some(idx) = self.idx_of(id) {
            return Some(self.data[idx].take().expect(LENT_ROW));
        }
        self.push(id, None);
        None
    }

    fn put(&mut self, id: WidgetId, value: T) {
        match self.idx_of(id) {
            Some(idx) => self.data[idx] = Some(value),
            None => self.push(id, Some(value)),
        }
    }

    #[track_caller]
    fn set(&mut self, id: WidgetId, value: T) {
        match self.idx_of(id) {
            Some(idx) => {
                let row = &mut self.data[idx];
                assert!(row.is_some(), "{LENT_ROW}");
                *row = Some(value);
            }
            None => self.push(id, Some(value)),
        }
    }

    #[track_caller]
    fn remove(&mut self, id: WidgetId) -> Option<T> {
        let idx = self.idx_of(id)?;
        let value = self.data[idx].take().expect(LENT_ROW);
        self.drop_row(id);
        Some(value)
    }

    /// Drop `id`'s row, if it has one: `swap_remove` it, then patch the swapped neighbor's index
    /// off `owners`.
    fn drop_row(&mut self, id: WidgetId) {
        let Some(idx) = self.map.remove(&id) else {
            return;
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

    fn push(&mut self, id: WidgetId, value: Option<T>) {
        let idx = self.data.len();
        debug_assert!(idx < u32::MAX as usize, "StateMap store overflow");
        self.data.push(value);
        self.owners.push(id);
        self.map.insert(id, idx as u32);
    }
}

impl<T: 'static> TypedStore<WidgetIdSet> for Store<T> {
    /// Drop the row of each removed widget. A row lent out is dropped as it is: a sweep runs at
    /// the end of a frame, when no body holds one.
    fn sweep(&mut self, removed: &WidgetIdSet) {
        #[expect(
            clippy::iter_over_hash_type,
            reason = "a fixed hasher, so the rows end in the same slots from run to run"
        )]
        for &id in removed {
            self.drop_row(id);
        }
    }

    /// Read only by the drop-drained sweep, which state doesn't use; an empty per-`T` store costs one hashmap slot and is reused.
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests;
