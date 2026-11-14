//! The per-`T` animation table, and the one tick that advances a row in it.

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
    pub(crate) render_frame_idx: u64,
}

impl<T: Animatable> AnimationMapTyped<T> {
    /// Insert-or-advance. First touch snaps to the target and returns settled: no animation on
    /// appearance. Later calls advance by `clock.dt` seconds.
    ///
    /// **A new target under the request's key is followed, not animated toward**: the row's
    /// offset from it and the clock running it carry on, so a look rides a theme transition with
    /// no delay while its own state fade runs. A new key, or no key, retargets.
    ///
    /// **`spec` is read only at a retarget**: a run in flight keeps the spec it started under
    /// (`MotionRow`'s decision), and a row at rest has nothing to run.
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
    /// The value to show this frame lands in `request.target`, which a settled row leaves as the
    /// caller passed it; returns whether the row is settled. In place, so a row at rest copies no
    /// look (`.notes/PERF.md`, "A look animates in place").
    ///
    /// # Panics
    ///
    /// For a target that is not finite, checked past the steady path, so a settled row pays
    /// nothing.
    #[track_caller]
    pub(crate) fn tick(
        &mut self,
        request: &mut AnimationRequest<T>,
        spec: AnimationSpec,
        clock: AnimationClock,
    ) -> bool {
        let id = request.id;
        let slot = request.slot;
        let key = request.key;
        let target = &mut request.target;
        let AnimationClock {
            dt,
            render_frame_idx,
        } = clock;
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimationRow {
                    target: target.clone(),
                    key,
                    motion: None,
                    ticked_at: render_frame_idx,
                });
                return true;
            }
            Entry::Occupied(o) => o.into_mut(),
        };
        let already_advanced = row.ticked_at == render_frame_idx;
        row.ticked_at = render_frame_idx;

        // Steady-state fast path: a resting row with the same target is a no-op, the caller's
        // target the value. Retarget detection still runs so a new target starts a motion.
        if row.motion.is_none() && row.target == *target {
            row.key = key;
            return true;
        }
        // Past the steady path, which a finite target takes and NaN, never equal to itself,
        // never does: such a target would retarget and repaint every frame. Through `sub`, so
        // `inf - inf` is caught too.
        assert!(
            target.sub(target).magnitude_squared().is_finite(),
            "animation target of {id:?} {slot:?} is not finite",
        );

        if key.is_some() && key == row.key && row.target != *target {
            let Some(motion) = &row.motion else {
                row.target = target.clone();
                return true;
            };
            let cur = motion.cur(target);
            row.target = target.clone();
            let step = row.advance_once(cur, already_advanced, dt);
            *target = step.value;
            return step.settled;
        }

        let old_key = row.key;
        row.key = key;
        let mut cur = row.cur();
        if row.target != *target {
            let from_rest = row.motion.is_none();
            // On the stack until it proves to run, so a target that drifts under the floor each
            // frame allocates nothing.
            let mut fresh = None;
            let motion = match &mut row.motion {
                Some(motion) => &mut **motion,
                None => fresh.insert(MotionRow::new(spec.motion, &cur, target)),
            };
            motion.retarget(
                &mut Retarget {
                    cur: &mut cur,
                    old_target: &row.target,
                    target,
                    old_key,
                    key,
                    at_rest: from_rest,
                },
                spec.motion,
            );
            // Snap-if-close, on the retarget alone: a change too small to see settles at
            // once. A running motion ends by its own rule; checked mid-curve this floor cut
            // `OutBack` short where it crosses the target toward the overshoot.
            let close = motion.close_enough(&animation_row::offset(&cur, target));
            row.target = target.clone();
            if close {
                row.motion = None;
                return true;
            }
            if let Some(motion) = fresh {
                row.motion = Some(Box::new(motion));
                *target = cur;
                return false;
            }
        }

        let step = row.advance_once(cur, already_advanced, dt);
        *target = step.value;
        step.settled
    }
}

impl<T: Animatable> TypedStore<u64> for AnimationMapTyped<T> {
    /// Keep the rows render frame `render_frame_idx` ticked. A read of each row, no write.
    fn sweep(&mut self, render_frame_idx: &u64) {
        self.rows
            .retain(|_, row| row.ticked_at == *render_frame_idx);
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
            let render_frame_idx = self
                .rows
                .values()
                .map(|row| row.ticked_at)
                .max()
                .unwrap_or(0)
                + 1;
            self.ticked(
                request,
                spec,
                AnimationClock {
                    dt,
                    render_frame_idx,
                },
            )
        }

        /// [`Self::tick`] of `request`, its value and whether it settled, on `clock`.
        pub(crate) fn ticked(
            &mut self,
            mut request: AnimationRequest<T>,
            spec: AnimationSpec,
            clock: AnimationClock,
        ) -> Animated<T> {
            let settled = self.tick(&mut request, spec, clock);
            Animated {
                value: request.target,
                settled,
            }
        }
    }
}
