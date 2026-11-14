//! Per-`(WidgetId, AnimationSlot)` animation rows, generic over [`Animatable`]. Storage is type-erased: [`AnimationMap`] holds one boxed [`AnimationMapTyped<T>`] per `TypeId` used, allocated by the first `Ui::animate::<T>` call.

pub(crate) mod animated;
pub(crate) mod animation_key;
pub(crate) mod animation_map_typed;
pub(crate) mod animation_request;
pub(crate) mod animation_row;
pub(crate) mod animation_slot;
pub(crate) mod animation_spec;
#[cfg(feature = "internals")]
pub(crate) mod bench;
mod duration;
pub(crate) mod easing;
pub(crate) mod motion_role;
pub(crate) mod paint_curve;
pub(crate) mod phase;
mod spring;

use crate::animation::animation_map_typed::{AnimationClock, AnimationMapTyped};
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::identity::typed_stores::{Drained, TypedStores};
use crate::primitives::math::animatable::Animatable;

/// Central animation table on [`crate::Ui`]; typed maps are allocated on demand by `TypeId`.
/// Each sweep reads the render frame id that just recorded.
#[derive(Debug, Default)]
pub(crate) struct AnimationMap {
    stores: TypedStores<u64>,
}

impl AnimationMap {
    /// Resolve one call site's animated value for this frame into `request.target`, and whether
    /// it is settled: an instant spec ([`AnimationSpec::is_instant`]) is the target, with no
    /// probe, and leaves a stale row unticked for the end-of-frame sweep to drop; otherwise tick.
    ///
    /// The caller owes the repaint when the result is unsettled.
    ///
    /// # Panics
    ///
    /// For a target that is not finite, as [`AnimationMapTyped::tick`] says.
    #[track_caller]
    pub(crate) fn animate<T: Animatable>(
        &mut self,
        request: &mut AnimationRequest<T>,
        spec: AnimationSpec,
        clock: AnimationClock,
    ) -> bool {
        if spec.is_instant() {
            return true;
        }
        self.typed_mut::<T>().tick(request, spec, clock)
    }

    /// Get-or-create the typed map for `T`.
    fn typed_mut<T: Animatable>(&mut self) -> &mut AnimationMapTyped<T> {
        self.stores.get_or_default::<AnimationMapTyped<T>>()
    }

    /// Drop every row that render frame `render_frame_idx` did not tick: a removed widget's, and a
    /// slot its caller stopped asking for. Once per recorded frame, from
    /// `WindowFrames::finalize_frame`; a row ticked under an id no widget recorded stays.
    ///
    /// A typed map that drains to empty is dropped ([`Drained::Drop`]), so an app gone idle holds none.
    pub(crate) fn sweep(&mut self, render_frame_idx: u64) {
        self.stores.sweep(&render_frame_idx, Drained::Drop);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::animation::AnimationMap;
    use crate::animation::animation_map_typed::AnimationMapTyped;
    use crate::primitives::math::animatable::Animatable;

    impl AnimationMap {
        /// No typed map is kept.
        pub(crate) fn is_empty(&self) -> bool {
            self.stores.is_empty()
        }

        /// Rows resident in the `T` map, zero when it has none.
        pub(crate) fn row_count<T: Animatable>(&self) -> usize {
            self.stores
                .get::<AnimationMapTyped<T>>()
                .map_or(0, AnimationMapTyped::len)
        }
    }
}

#[cfg(test)]
mod tests;
