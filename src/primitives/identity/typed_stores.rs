//! One `TypeId`-keyed container of type-erased per-widget stores.

use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::fmt;

/// Spelled once rather than at each downcast site (here and [`Singletons`](crate::ui::singletons::Singletons)): the `TypeId` key means nothing but an `S` can be behind it.
pub(crate) const DOWNCAST_ERROR: &str = "TypeId keys the entry, so the stored type is S";

/// What a typed store owes its container: the end-of-frame sweep against what the frame left in
/// `F`, and an emptiness probe. `: Any` lets downcast sites upcast to `&(mut) dyn Any` directly.
pub(crate) trait TypedStore<F: ?Sized>: Any {
    fn sweep(&mut self, frame: &F);
    fn is_empty(&self) -> bool;
}

/// One boxed store per distinct payload type; the shared half of `StateMap` and `AnimationMap`, whose stores differ in shape and in what their sweep reads (`F`) but share the `TypeId` probe, downcast and end-of-frame sweep.
pub(crate) struct TypedStores<F: ?Sized + 'static> {
    by_type: FxHashMap<TypeId, Box<dyn TypedStore<F>>>,
}

impl<F: ?Sized> Default for TypedStores<F> {
    fn default() -> Self {
        Self {
            by_type: FxHashMap::default(),
        }
    }
}

// Manual: `dyn TypedStore` has no `Debug`; the store count is the shape worth reporting.
impl<F: ?Sized> fmt::Debug for TypedStores<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypedStores")
            .field("stores", &self.by_type.len())
            .finish_non_exhaustive()
    }
}

impl<F: ?Sized> TypedStores<F> {
    /// The store for `S`, created empty on first use.
    pub(crate) fn get_or_default<S: TypedStore<F> + Default>(&mut self) -> &mut S {
        (self
            .by_type
            .entry(TypeId::of::<S>())
            .or_insert_with(|| Box::<S>::default())
            .as_mut() as &mut dyn Any)
            .downcast_mut::<S>()
            .expect(DOWNCAST_ERROR)
    }

    /// The store for `S` if one exists; `None` means no caller has reached for `S` yet.
    pub(crate) fn get<S: TypedStore<F>>(&self) -> Option<&S> {
        self.by_type.get(&TypeId::of::<S>()).map(|store| {
            (store.as_ref() as &dyn Any)
                .downcast_ref::<S>()
                .expect(DOWNCAST_ERROR)
        })
    }

    /// Mutable [`Self::get`]; does not create the store.
    pub(crate) fn get_mut<S: TypedStore<F>>(&mut self) -> Option<&mut S> {
        self.by_type.get_mut(&TypeId::of::<S>()).map(|store| {
            (store.as_mut() as &mut dyn Any)
                .downcast_mut::<S>()
                .expect(DOWNCAST_ERROR)
        })
    }

    /// Sweep every store against `frame`, keeping or dropping drained ones per `drained`; one walk with the policy as an argument, so the difference stays visible at the call site.
    pub(crate) fn sweep(&mut self, frame: &F, drained: Drained) {
        self.by_type.retain(|_, store| {
            store.sweep(frame);
            drained == Drained::Keep || !store.is_empty()
        });
    }
}

/// What a sweep does with a store that drained to empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Drained {
    /// Keep it: an empty per-`T` store costs one slot and is reused, which suits a few long-lived types.
    Keep,
    /// Drop it, so an idle table holds no store; otherwise one ever-used type keeps one.
    Drop,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::typed_stores::TypedStores;

    impl<F: ?Sized> TypedStores<F> {
        /// No store exists for any type.
        pub(crate) fn is_empty(&self) -> bool {
            self.by_type.is_empty()
        }
    }
}
