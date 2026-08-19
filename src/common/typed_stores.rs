//! One `TypeId`-keyed container of type-erased per-widget stores.

use crate::primitives::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::any::{Any, TypeId};

/// Spelled once rather than at each of the three downcast sites: the
/// argument is the same one every time — the entry is keyed by
/// `TypeId::of::<S>()`, so nothing but an `S` can be behind it — and
/// three copies of it are three chances to weaken one.
const DOWNCAST_ERROR: &str = "TypeId keys the entry, so the stored type is S";

/// What a typed store owes the container holding it: the end-of-frame
/// sweep, and an emptiness probe for the tables that drop drained
/// stores.
///
/// `: Any` is what lets the downcast sites upcast a `&(mut) dyn
/// TypedStore` straight to `&(mut) dyn Any` — no `as_any` boilerplate.
pub(crate) trait TypedStore: Any {
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>);
    fn is_empty(&self) -> bool;
}

/// One boxed store per distinct payload type.
///
/// The shared half of `StateMap` and `AnimMap`. Both hold per-widget
/// rows in a store whose *shape* is their own — one a dense `Vec`
/// indexed by `WidgetId`, the other a row list keyed by
/// `(WidgetId, AnimSlot)` — but both reach that store through the same
/// `TypeId` probe and the same downcast, and both fan the same
/// end-of-frame sweep across it. That half is written here once instead
/// of twice, which is what keeps the two from drifting on the downcast's
/// safety argument.
#[derive(Default)]
pub(crate) struct TypedStores {
    by_type: FxHashMap<TypeId, Box<dyn TypedStore>>,
}

// Manual: the values are `dyn TypedStore`, which has no `Debug` and
// can't gain one without a supertrait every store would have to satisfy.
// The store count is the shape worth reporting.
impl std::fmt::Debug for TypedStores {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedStores")
            .field("stores", &self.by_type.len())
            .finish_non_exhaustive()
    }
}

impl TypedStores {
    /// No store exists for any type yet — the fast path for a table
    /// nothing has touched.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// The store for `S`, created empty on first use.
    pub(crate) fn get_or_default<S: TypedStore + Default>(&mut self) -> &mut S {
        (self
            .by_type
            .entry(TypeId::of::<S>())
            .or_insert_with(|| Box::<S>::default())
            .as_mut() as &mut dyn Any)
            .downcast_mut::<S>()
            .expect(DOWNCAST_ERROR)
    }

    /// The store for `S` if one exists. `None` means no caller has
    /// reached for `S` yet — never a type mismatch, which the `TypeId`
    /// key rules out.
    pub(crate) fn get<S: TypedStore>(&self) -> Option<&S> {
        self.by_type.get(&TypeId::of::<S>()).map(|store| {
            (store.as_ref() as &dyn Any)
                .downcast_ref::<S>()
                .expect(DOWNCAST_ERROR)
        })
    }

    /// Mutable [`Self::get`]. Does **not** create the store, so a probe
    /// that misses leaves the table untouched.
    pub(crate) fn get_mut<S: TypedStore>(&mut self) -> Option<&mut S> {
        self.by_type.get_mut(&TypeId::of::<S>()).map(|store| {
            (store.as_mut() as &mut dyn Any)
                .downcast_mut::<S>()
                .expect(DOWNCAST_ERROR)
        })
    }

    /// Sweep every store, keeping them all — for a table whose empty
    /// stores cost nothing to hold.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        for store in self.by_type.values_mut() {
            store.sweep_removed(removed);
        }
    }

    /// Sweep every store and drop the ones that drained, so
    /// [`Self::is_empty`] goes back to being a real fast path once the
    /// table goes idle. Without it a single ever-used type would leave
    /// the container non-empty forever.
    pub(crate) fn sweep_removed_dropping_drained(&mut self, removed: &FxHashSet<WidgetId>) {
        self.by_type.retain(|_, store| {
            store.sweep_removed(removed);
            !store.is_empty()
        });
    }
}
