//! The per-`T` animation table, and the one tick that advances a row in it.

use crate::animation::animated::Animated;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_row::{self, AnimationRow, MotionRow, Retarget};
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::identity::typed_stores::TypedStore;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::animatable::Animatable;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Per-`T` animation table inside [`AnimationMap`](crate::animation::AnimationMap), a boxed
/// trait object keyed by `TypeId`, allocated on first `Ui::animate::<T>`.
#[derive(Debug)]
pub(crate) struct AnimationMapTyped<T: Animatable> {
    pub(super) rows: FxHashMap<(WidgetId, AnimationSlot), AnimationRow<T>>,
}

impl<T: Animatable> Default for AnimationMapTyped<T> {
    fn default() -> Self {
        Self {
            rows: FxHashMap::default(),
        }
    }
}

/// The frame's animation step: the seconds a row advances by, and the render frame spending
/// them, so a second record pass of one frame does not spend them again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AnimationClock {
    pub(crate) dt: f32,
    pub(crate) render_frame_id: u64,
}

impl<T: Animatable> AnimationMapTyped<T> {
    /// Insert-or-advance. First touch snaps `cur = target` and returns settled:
    /// no animation on appearance. Later calls detect retarget vs steady state and
    /// advance by `clock.dt` seconds.
    ///
    /// **A new target under the request's key is followed, not animated toward**: the row's
    /// offset from it and the clock running it carry on, so a look rides a theme transition with
    /// no delay while its own state fade runs. A new key, or no key, retargets.
    ///
    /// **A motion starting from rest spends nothing on its first frame**, as a CSS
    /// transition shows its start value: the `dt` before it passed while the row
    /// was at rest, and after an idle window spending it would use up most of
    /// `AnimationSpec::FAST` before anything painted. A row in flight spends `dt`
    /// as usual.
    ///
    /// `AnimationMap::animate` filters instant specs (`AnimationSpec::is_instant()`) first;
    /// this assumes a real motion spec.
    ///
    /// # Panics
    ///
    /// For a target that is not finite, checked past the steady path, so a settled row pays
    /// nothing.
    #[track_caller]
    pub(crate) fn tick(
        &mut self,
        request: AnimationRequest<T>,
        spec: AnimationSpec,
        clock: AnimationClock,
    ) -> Animated<T> {
        let AnimationRequest {
            id,
            slot,
            target,
            key,
        } = request;
        let AnimationClock {
            dt,
            render_frame_id,
        } = clock;
        // `T: Animatable` is `Clone`, not `Copy`: consuming a T field needs an explicit
        // `.clone()`, free for Copy types and a deliberate memcpy for heavy ones.
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimationRow {
                    target: target.clone(),
                    key,
                    motion: MotionRow::new(spec.motion, &target, &target),
                    ticked_at: render_frame_id,
                    settled: true,
                });
                return Animated {
                    value: target,
                    settled: true,
                };
            }
            Entry::Occupied(o) => o.into_mut(),
        };
        let already_advanced = row.ticked_at == render_frame_id;
        row.ticked_at = render_frame_id;

        // Steady-state fast path: a settled row with the same target is a no-op, so
        // skip the settle math. Retarget detection still runs so a new target unfreezes
        // the row.
        //
        // Returns the caller's `target` instead of `row.cur()`: every `settled`
        // site leaves the motion at rest on the target, so this skips a clone per
        // widget per frame. The assert is debug-only since the compare is the cost
        // this path avoids.
        if row.settled && row.target == target {
            debug_assert!(row.cur() == target, "settled row must sit at its target");
            row.key = key;
            return Animated {
                value: target,
                settled: true,
            };
        }
        // Past the steady path, which a finite target takes and NaN, never equal to itself,
        // never does: such a target would retarget and repaint every frame. Through `sub`, so
        // `inf - inf` is caught too.
        assert!(
            target
                .clone()
                .sub(target.clone())
                .magnitude_squared()
                .is_finite(),
            "animation target of {id:?} {slot:?} is not finite",
        );

        if key.is_some() && key == row.key && row.target != target {
            if row.settled {
                debug_assert!(
                    row.motion.cur(&target) == target,
                    "a settled row holds no offset, so it rests on a moved target"
                );
                let cur = target.clone();
                row.target = target;
                return Animated {
                    value: cur,
                    settled: true,
                };
            }
            let cur = row.motion.cur(&target);
            row.motion.conform(spec.motion, &cur, &target);
            row.target = target;
            return row.advance_once(cur, already_advanced, dt);
        }

        let old_key = row.key;
        row.key = key;
        let retargeted = row.target != target;
        let mut cur = row.cur();
        let switched = row.motion.conform(spec.motion, &cur, &row.target);
        // Only a new target or motion can pair the value with a target it can't do
        // arithmetic against; otherwise every value is `target + offset`.
        if retargeted || switched {
            if let MotionRow::Spring { velocity, .. } = &mut row.motion {
                cur.normalize_for_spring(&target, velocity);
            }
            row.motion.retarget(&Retarget {
                cur: &cur,
                old_target: &row.target,
                target: &target,
                old_key,
                key,
                at_rest: row.settled,
            });
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
                .close_enough(animation_row::offset(&cur, &row.target))
            {
                row.motion.stop();
                row.settled = true;
                return Animated {
                    value: row.target.clone(),
                    settled: true,
                };
            }
            if from_rest {
                return Animated {
                    value: cur,
                    settled: false,
                };
            }
        }

        row.advance_once(cur, already_advanced, dt)
    }
}

impl<T: Animatable> TypedStore<u64> for AnimationMapTyped<T> {
    /// Keep the rows render frame `render_frame_id` ticked. A read of each row, no write.
    fn sweep(&mut self, render_frame_id: &u64) {
        self.rows.retain(|_, row| row.ticked_at == *render_frame_id);
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
    use crate::animation::animated::Animated;
    use crate::animation::animation_map_typed::{AnimationClock, AnimationMapTyped};
    use crate::animation::animation_request::AnimationRequest;
    use crate::animation::animation_spec::AnimationSpec;
    use crate::primitives::math::animatable::Animatable;

    impl<T: Animatable> AnimationMapTyped<T> {
        pub(crate) fn len(&self) -> usize {
            self.rows.len()
        }

        /// [`Self::tick`] on a render frame after every row's last tick, for tests that don't
        /// care about pass A/B.
        pub(crate) fn step(
            &mut self,
            request: AnimationRequest<T>,
            spec: AnimationSpec,
            dt: f32,
        ) -> Animated<T> {
            let render_frame_id = self
                .rows
                .values()
                .map(|row| row.ticked_at)
                .max()
                .unwrap_or(0)
                + 1;
            let clock = AnimationClock {
                dt,
                render_frame_id,
            };
            self.tick(request, spec, clock)
        }
    }
}
