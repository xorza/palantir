//! The per-`T` animation table, and the one tick that advances a row in it.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::animatable::Animatable;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::common::typed_stores::TypedStore;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Per-`T` animation table inside [`AnimMap`](crate::animation::AnimMap), a boxed
/// trait object keyed by `TypeId`, allocated on first `Ui::animate::<T>`.
#[derive(Debug)]
pub(crate) struct AnimMapTyped<T: Animatable> {
    pub(super) rows: FxHashMap<(WidgetId, AnimationSlot), AnimRow<T>>,
}

impl<T: Animatable> Default for AnimMapTyped<T> {
    fn default() -> Self {
        Self {
            rows: FxHashMap::default(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct TickResult<T: Animatable> {
    pub(crate) current: T,
    pub(crate) settled: bool,
}

impl<T: Animatable> AnimMapTyped<T> {
    /// Forget `id`'s `slot`, so a later call starts fresh from its target.
    pub(super) fn drop_row(&mut self, id: WidgetId, slot: AnimationSlot) {
        self.rows.remove(&(id, slot));
    }

    /// Insert-or-advance. First touch snaps `current = target` and returns settled:
    /// no animation on appearance. Later calls detect retarget vs steady state and
    /// advance by `dt` seconds.
    ///
    /// **A motion starting from rest spends nothing on its first frame**, as a CSS
    /// transition shows its start value: the `dt` before it passed while the row
    /// was at rest, and after an idle window spending it would use up most of
    /// `AnimationSpec::FAST` before anything painted. A row in flight spends `dt`
    /// as usual.
    ///
    /// `Ui::animate` filters instant specs (`AnimationSpec::is_instant()`) first;
    /// this assumes a real motion spec.
    pub(crate) fn tick(
        &mut self,
        id: WidgetId,
        slot: AnimationSlot,
        target: T,
        spec: AnimationSpec,
        dt: f32,
        render_frame_id: u64,
    ) -> TickResult<T> {
        // A non-finite target is a logic error and would never settle (NaN != NaN
        // retargets every frame). Through `sub` so `inf - inf` is caught too.
        debug_assert!(
            target
                .clone()
                .sub(target.clone())
                .magnitude_squared()
                .is_finite(),
            "animation target of {id:?} {slot:?} is not finite",
        );
        // `T: Animatable` is `Clone`, not `Copy`: consuming a T field needs an explicit
        // `.clone()`, free for Copy types and a deliberate memcpy for heavy ones.
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimRow {
                    target: target.clone(),
                    motion: MotionRow::new(spec.motion, &target, &target),
                    touched: true,
                    advanced_at: render_frame_id,
                    settled: true,
                });
                return TickResult {
                    current: target,
                    settled: true,
                };
            }
            Entry::Occupied(o) => o.into_mut(),
        };
        row.touched = true;
        let already_advanced = row.advanced_at == render_frame_id;
        row.advanced_at = render_frame_id;

        // Steady-state fast path: a settled row with the same target is a no-op, so
        // skip the settle math. Retarget detection still runs so a new target unfreezes
        // the row.
        //
        // Returns the caller's `target` instead of `row.current()`: every `settled`
        // site leaves the motion at rest on the target, so this skips a clone per
        // widget per frame. The assert is debug-only since the compare is the cost
        // this path avoids.
        if row.settled && row.target == target {
            debug_assert!(
                row.current() == target,
                "settled row must sit at its target"
            );
            return TickResult {
                current: target,
                settled: true,
            };
        }

        let retargeted = row.target != target;
        let mut current = row.current();
        let switched = row.motion.conform(spec.motion, &current, &row.target);
        // Only a new target or motion can pair the value with a target it can't do
        // arithmetic against; otherwise every value is `target + offset` or a curve
        // sample.
        if retargeted || switched {
            if let MotionRow::Spring { velocity, .. } = &mut row.motion {
                current.normalize_for_spring(&target, velocity);
            }
            row.motion.retarget(&current, &target);
        }

        if retargeted {
            let from_rest = row.settled;
            row.target = target;
            row.settled = false;
            // Snap-if-close, on the retarget alone: a change too small to see settles at
            // once. A running motion ends by its own rule; checked mid-curve this floor cut
            // `OutBack` short where it crosses the target toward the overshoot.
            if row
                .motion
                .close_enough(current.clone().sub(row.target.clone()))
            {
                row.motion.stop(&row.target);
                row.settled = true;
                return TickResult {
                    current: row.target.clone(),
                    settled: true,
                };
            }
            if from_rest {
                return TickResult {
                    current,
                    settled: false,
                };
            }
        }

        // Multi-pass guard: pass A already advanced the integrator this frame, and
        // pass B only updated target / `segment_start` / `velocity`; adding another dt
        // would double the speed on any input frame.
        if already_advanced {
            return TickResult {
                current,
                settled: false,
            };
        }

        let step = row.motion.advance(row.target.clone(), dt);
        row.settled = step.settled;
        step
    }
}

impl<T: Animatable> TypedStore for AnimMapTyped<T> {
    /// Drop rows for removed widgets and slots the caller stopped poking this
    /// frame; clear `touched` on survivors. One retain pass.
    fn sweep_removed(&mut self, removed: &WidgetIdSet) {
        self.rows.retain(|(id, _), row| {
            if removed.contains(id) {
                return false;
            }
            let kept = row.touched;
            row.touched = false;
            kept
        });
    }
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Reach-ins for the animation tests: the resident row count the eviction tests
/// assert reaches zero, and a tick on a fresh render frame. `cfg(test)` alone,
/// since benches compile under `internals` without the harness rung that reads
/// the count.
#[cfg(test)]
pub(crate) mod internals {
    use crate::animation::anim_map_typed::{AnimMapTyped, TickResult};
    use crate::animation::animatable::Animatable;
    use crate::animation::animation_slot::AnimationSlot;
    use crate::animation::animation_spec::AnimationSpec;
    use crate::primitives::identity::widget_id::WidgetId;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A render frame id no earlier call returned, so the multi-pass guard never
    /// short-circuits a [`AnimMapTyped::step`].
    fn next_frame() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed) + 1
    }

    impl<T: Animatable> AnimMapTyped<T> {
        pub(crate) fn len(&self) -> usize {
            self.rows.len()
        }

        /// [`Self::tick`] on a render frame of its own, for tests that don't care about
        /// pass A/B.
        pub(crate) fn step(
            &mut self,
            id: WidgetId,
            slot: AnimationSlot,
            target: T,
            spec: AnimationSpec,
            dt: f32,
        ) -> TickResult<T> {
            self.tick(id, slot, target, spec, dt, next_frame())
        }
    }
}
