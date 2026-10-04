//! The per-`T` animation table, and the one tick that advances a row in it.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::animatable::Animatable;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::common::typed_stores::TypedStore;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Per-`T` animation table. Lives inside [`AnimMap`](crate::animation::AnimMap) behind a boxed
/// trait object keyed by `TypeId`; allocated on first
/// `Ui::animate::<T>` call.
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
    /// Forget `id`'s `slot`, so a later animated call starts fresh from
    /// whatever target it is given. The key shape is this table's own,
    /// which is why no caller spells it.
    pub(super) fn drop_row(&mut self, id: WidgetId, slot: AnimationSlot) {
        self.rows.remove(&(id, slot));
    }

    /// Insert-or-advance. First touch snaps `current = target` and
    /// returns settled — there's no animation on appearance, by
    /// design. Subsequent calls detect retarget vs steady-state and
    /// advance by `dt` seconds.
    ///
    /// **A motion that starts from rest spends nothing on its first
    /// frame**, as a CSS transition shows its start value on the frame of
    /// the change: the `dt` before it is time that passed while the row was
    /// at rest — after an idle window, the whole 0.1 s clamp, and spending
    /// it would use up 99.5 % of `AnimationSpec::FAST` before anything was
    /// painted. A row already in flight spends `dt` as usual, so a target
    /// that moves every frame keeps moving.
    ///
    /// Caller (`Ui::animate`) is responsible for filtering instant
    /// specs (`AnimationSpec::is_instant()`) before calling this — tick
    /// itself assumes a real motion spec, no degenerate cases.
    pub(crate) fn tick(
        &mut self,
        id: WidgetId,
        slot: AnimationSlot,
        target: T,
        spec: AnimationSpec,
        dt: f32,
        render_frame_id: u64,
    ) -> TickResult<T> {
        // A non-finite target is a caller's logic error, and would never
        // settle: NaN differs from itself, so the row would retarget every
        // frame forever. Through `sub`, so `inf - inf` is caught too.
        debug_assert!(
            target
                .clone()
                .sub(target.clone())
                .magnitude_squared()
                .is_finite(),
            "animation target of {id:?} {slot:?} is not finite",
        );
        // `T: Animatable` is `Clone` (not `Copy`): each consume of a
        // T field through trait methods needs an explicit `.clone()`.
        // For Copy fields (f32, Vec2, RgbaF32) the clone compiles away;
        // for heavyweights (Background) the clone is a deliberate
        // memcpy at a known site.
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

        // Steady-state fast path. Once a row settles, every subsequent
        // tick with the same target should be a no-op — skip the
        // `sub` + `settle_distance_squared` settle math entirely. Retarget
        // detection still runs (the `target != row.target` compare
        // below) so a caller changing the target unfreezes the row
        // immediately.
        //
        // Returns the caller's `target` instead of `row.current()`: every
        // site that sets `settled` also leaves the motion at rest on the
        // target, so the three values are equal here and reusing the
        // already-owned `target` skips a per-widget-per-frame clone
        // (~200 B for `AnimatedLook`). debug-only assert — the compare
        // is exactly the cost this path exists to avoid.
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
        // Only a new target or a new motion can pair the value with a
        // target it cannot do arithmetic against: between the two, every
        // value is `target + offset` or a sample of the curve toward it.
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
            // Snap-if-close, on the retarget alone: a change too small to
            // see settles at once rather than running a full curve for it.
            // A running motion ends by its own rule — checked mid-curve,
            // this floor cut `OutBack` short where the curve crosses the
            // target on its way to the overshoot.
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

        // Multi-pass guard: pass A already advanced the integrator
        // this frame. Pass B's retarget logic (above) updated `target`
        // / `segment_start` / `velocity` for the new post-action
        // state, but we don't add another dt of motion — that would
        // double the animation speed on any input frame.
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
    /// Drop rows for any removed widget *and* any slot whose caller
    /// stopped poking it this frame; clear the `touched` flag on the
    /// rows that survive. Single retain pass — both predicates fold
    /// into one walk.
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

/// Reach-ins for the animation tests: the resident row count the
/// eviction tests assert the sweep drives to zero, and a tick on a fresh
/// render frame. `cfg(test)` alone, because the harness rung that reads
/// the count is `cfg(test)` too — the benches compile under `internals`
/// without it.
#[cfg(test)]
pub(crate) mod internals {
    use crate::animation::anim_map_typed::{AnimMapTyped, TickResult};
    use crate::animation::animatable::Animatable;
    use crate::animation::animation_slot::AnimationSlot;
    use crate::animation::animation_spec::AnimationSpec;
    use crate::primitives::identity::widget_id::WidgetId;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A render frame id no earlier call returned, so the multi-pass
    /// guard never short-circuits a [`AnimMapTyped::step`].
    fn next_frame() -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed) + 1
    }

    impl<T: Animatable> AnimMapTyped<T> {
        pub(crate) fn len(&self) -> usize {
            self.rows.len()
        }

        /// [`Self::tick`] on a render frame of its own — for a test that
        /// does not care about pass A/B. A test of the multi-pass guard
        /// passes its frame ids to `tick` itself.
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
