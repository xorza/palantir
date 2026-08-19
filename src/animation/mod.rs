//! Per-`(WidgetId, AnimSlot)` animation rows, generic over
//! [`Animatable`].
//!
//! Storage is type-erased: [`AnimMap`] holds one boxed
//! [`AnimMapTyped<T>`] per `TypeId` actually used. Adding a new
//! `Animatable` type costs no central edits — first call to
//! `Ui::animate::<T>` allocates the typed slot on demand.
//! `#[derive(Animatable)]` from `palantir-anim-derive` wires the
//! math; this module wires the storage.

pub(crate) mod anim_map_typed;
pub(crate) mod anim_row;
pub(crate) mod anim_slot;
pub(crate) mod anim_spec;
pub(crate) mod animatable;
#[cfg(feature = "bench")]
pub(crate) mod bench;
mod duration;
pub(crate) mod easing;
mod spring;

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::animatable::Animatable;
use crate::common::typed_stores::TypedStores;
use crate::primitives::widget_id::WidgetId;
use rustc_hash::FxHashSet;

/// Central animation table on [`crate::Ui`]. Typed maps allocated on demand
/// keyed by `TypeId`. Adding a new [`Animatable`] type costs no
/// central edits — first `Ui::animate::<T>` call boxes a fresh
/// `AnimMapTyped<T>`.
#[derive(Debug, Default)]
pub(crate) struct AnimMap {
    stores: TypedStores,
}

impl AnimMap {
    /// Get-or-create the typed map for `T`. Allocates on first call
    /// per `T`; subsequent calls hit the hashmap and downcast.
    pub(crate) fn typed_mut<T: Animatable>(&mut self) -> &mut AnimMapTyped<T> {
        self.stores.get_or_default::<AnimMapTyped<T>>()
    }

    /// No typed map exists yet — the `Ui::animate` fast path for an app
    /// that has never animated, and again once every map has drained.
    pub(crate) fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    /// Borrow the typed map for `T` if it exists. Used by the
    /// `Ui::animate(.., None)` short-circuit to drop a stale row
    /// without allocating a fresh typed map.
    pub(crate) fn try_typed_mut<T: Animatable>(&mut self) -> Option<&mut AnimMapTyped<T>> {
        self.stores.get_mut::<AnimMapTyped<T>>()
    }

    /// Drop rows for removed widgets and for slots that weren't
    /// poked this frame, then clear the `touched` flags on the rows
    /// that survive. Called from `FrameCycle::finalize_frame` once per frame; the
    /// `removed` set is the same one that drives `StateMap` / text /
    /// layout sweeps. A `(WidgetId, AnimSlot)` row goes away if
    /// either (a) the widget itself disappeared or (b) the call site
    /// that owns the slot stopped reaching for it — without (b),
    /// abandoned slots would accumulate forever for any widget
    /// whose id lingers across motion-toggle states.
    ///
    /// A typed map that drains to empty is dropped entirely — see
    /// [`TypedStores::sweep_removed_dropping_drained`]. Keeping it would
    /// leave the container non-empty forever, permanently disabling the
    /// [`Self::is_empty`] fast path in `Ui::animate` once *any* widget
    /// has ever animated, even after the app goes idle.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        self.stores.sweep_removed_dropping_drained(removed);
    }
}

#[cfg(test)]
mod tests;
