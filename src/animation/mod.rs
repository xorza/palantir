//! Per-`(WidgetId, AnimationSlot)` animation rows, generic over [`Animatable`]. Storage is type-erased: [`AnimMap`] holds one boxed [`AnimMapTyped<T>`] per `TypeId` used, allocated by the first `Ui::animate::<T>` call.

pub(crate) mod anim_map_typed;
pub(crate) mod anim_row;
pub(crate) mod animatable;
pub(crate) mod animation_slot;
pub(crate) mod animation_spec;
#[cfg(feature = "bench")]
pub(crate) mod bench;
mod duration;
pub(crate) mod easing;
mod spring;

use crate::animation::anim_map_typed::{AnimMapTyped, TickResult};
use crate::animation::animatable::Animatable;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::common::typed_stores::{Drained, TypedStores};
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};

/// Central animation table on [`crate::Ui`]; typed maps are allocated on demand by `TypeId`.
#[derive(Debug, Default)]
pub(crate) struct AnimMap {
    stores: TypedStores,
}

impl AnimMap {
    /// Resolve one call site's animated value for this frame: snap to
    ///
    /// Cheapest first: nothing has ever animated and no motion wanted returns `target` before any probe; a degenerate spec (`None` or ≈0 `Duration`) drops any stale row without allocating a typed map; otherwise tick.
    ///
    /// The caller owes the repaint when the result is unsettled.
    pub(crate) fn animate<T: Animatable>(
        &mut self,
        id: WidgetId,
        slot: impl Into<AnimationSlot>,
        target: T,
        spec: Option<AnimationSpec>,
        dt: f32,
        frame: u64,
    ) -> TickResult<T> {
        if self.is_empty() && spec.is_none_or(AnimationSpec::is_instant) {
            return TickResult {
                current: target,
                settled: true,
            };
        }
        let slot = slot.into();
        let Some(spec) = spec.filter(|s| !s.is_instant()) else {
            if let Some(typed) = self.try_typed_mut::<T>() {
                typed.drop_row(id, slot);
            }
            return TickResult {
                current: target,
                settled: true,
            };
        };
        self.typed_mut::<T>()
            .tick(id, slot, target, spec, dt, frame)
    }

    /// Get-or-create the typed map for `T`.
    fn typed_mut<T: Animatable>(&mut self) -> &mut AnimMapTyped<T> {
        self.stores.get_or_default::<AnimMapTyped<T>>()
    }

    /// No typed map exists yet: the fast path for an app that never animated, and once every map has drained.
    fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    /// Borrow the typed map for `T` if it exists, so `Ui::animate(.., None)` can drop a stale row without allocating.
    pub(crate) fn try_typed_mut<T: Animatable>(&mut self) -> Option<&mut AnimMapTyped<T>> {
        self.stores.get_mut::<AnimMapTyped<T>>()
    }

    /// Drop rows for removed widgets and for slots not poked this frame (else abandoned slots accumulate for widgets whose id lingers), then clear `touched` on survivors. Called once per frame from `FrameCycle::finalize_frame`.
    ///
    /// A typed map that drains to empty is dropped ([`Drained::Drop`]), or [`Self::is_empty`]'s fast path would stay disabled after the app goes idle.
    pub(crate) fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        self.stores.sweep_removed(removed, Drained::Drop);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::animation::AnimMap;
    use crate::animation::animatable::Animatable;

    impl AnimMap {
        /// Rows resident in the `T` map, zero when it has none.
        pub(crate) fn row_count<T: Animatable>(&mut self) -> usize {
            self.try_typed_mut::<T>().map_or(0, |rows| rows.len())
        }
    }
}

#[cfg(test)]
mod tests;
