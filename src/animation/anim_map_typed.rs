//! The per-`T` animation table, and the one tick that advances a row in it.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::{AnimMotion, AnimSpec};
use crate::animation::animatable::Animatable;
use crate::animation::duration::within_duration_snap_eps;
use crate::animation::spring::{step as spring_step, within_settle_eps};
use crate::common::typed_stores::TypedStore;
use crate::primitives::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;

/// Per-`T` animation table. Lives inside [`AnimMap`](crate::animation::AnimMap) behind a boxed
/// trait object keyed by `TypeId`; allocated on first
/// `Ui::animate::<T>` call.
#[derive(Debug)]
pub(crate) struct AnimMapTyped<T: Animatable> {
    pub(crate) rows: FxHashMap<(WidgetId, AnimSlot), AnimRow<T>>,
}

impl<T: Animatable> Default for AnimMapTyped<T> {
    fn default() -> Self {
        Self {
            rows: FxHashMap::default(),
        }
    }
}

/// Dot product via the polarization identity
/// `2·a·b = |a+b|² − |a|² − |b|²`, expressed in the existing
/// `Animatable` vocabulary (add + magnitude_squared) so we don't have
/// to widen the trait. Used only on spring retarget to decide whether
/// residual velocity aids or opposes motion toward the new target.
#[inline]
fn dot<T: Animatable>(a: T, b: T) -> f32 {
    // T is `Clone` (not `Copy`); each `Animatable` method consumes its
    // operand. Compute the magnitudes off the clones first, then let
    // `add` consume `a` and `b`.
    let mag_a = a.clone().magnitude_squared();
    let mag_b = b.clone().magnitude_squared();
    let sum = a.add(b).magnitude_squared();
    0.5 * (sum - mag_a - mag_b)
}

#[derive(Debug)]
pub(crate) struct TickResult<T: Animatable> {
    pub(crate) current: T,
    pub(crate) settled: bool,
}

impl<T: Animatable> AnimMapTyped<T> {
    /// Insert-or-advance. First touch snaps `current = target` and
    /// returns settled — there's no animation on appearance, by
    /// design. Subsequent calls detect retarget vs steady-state and
    /// advance by `dt` seconds.
    ///
    /// Caller (`Ui::animate`) is responsible for filtering instant
    /// specs (`AnimSpec::is_instant()`) before calling this — tick
    /// itself assumes a real motion spec, no degenerate cases.
    pub(crate) fn tick(
        &mut self,
        id: WidgetId,
        slot: AnimSlot,
        target: T,
        spec: AnimSpec,
        dt: f32,
        render_frame_id: u64,
    ) -> TickResult<T> {
        // `T: Animatable` is `Clone` (not `Copy`): each consume of a
        // T field through trait methods needs an explicit `.clone()`.
        // For Copy fields (f32, Vec2, Color) the clone compiles away;
        // for heavyweights (Background) the clone is a deliberate
        // memcpy at a known site.
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimRow {
                    current: target.clone(),
                    target: target.clone(),
                    motion: MotionRow::new(spec.motion, &target),
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

        let same_motion = matches!(
            (&row.motion, spec.motion),
            (MotionRow::Duration { .. }, AnimMotion::Duration { .. })
                | (MotionRow::Spring { .. }, AnimMotion::Spring { .. })
        );
        if !same_motion {
            row.motion = MotionRow::new(spec.motion, &row.current);
        }

        // Steady-state fast path. Once a row settles, every subsequent
        // tick with the same target should be a no-op — skip the
        // `sub` + `magnitude_squared` settle math entirely. Retarget
        // detection still runs (the `target != row.target` compare
        // below) so a caller changing the target unfreezes the row
        // immediately.
        //
        // Returns the caller's `target` instead of `row.current.clone()`:
        // every site that sets `settled` also snaps `current = target`,
        // so the three values are equal here and reusing the
        // already-owned `target` skips a per-widget-per-frame clone
        // (~200 B for `AnimatedLook`). debug-only assert — the compare
        // is exactly the cost this path exists to avoid.
        if row.settled && row.target == target {
            debug_assert!(row.current == target, "settled row must sit at its target");
            return TickResult {
                current: target,
                settled: true,
            };
        }

        if let MotionRow::Spring { velocity } = &mut row.motion {
            row.current.normalize_for_spring(&target, velocity);
        }

        // Retarget: duration restarts the segment from `current`;
        // spring keeps velocity *only when it aids motion toward the
        // new target* — preserves "fling through" continuations but
        // kills reversal swings that would otherwise overshoot far
        // past the new target (e.g. retargeting a toggle while the
        // spring is mid-flight in the opposite direction).
        // `Animatable: PartialEq` lets us short-circuit with a
        // bytewise compare on the steady-state path.
        if row.target != target {
            match &mut row.motion {
                MotionRow::Duration {
                    segment_start,
                    elapsed,
                } => {
                    *segment_start = row.current.clone();
                    *elapsed = 0.0;
                }
                MotionRow::Spring { velocity } => {
                    let to_target = target.clone().sub(row.current.clone());
                    if dot(velocity.clone(), to_target) < 0.0 {
                        *velocity = T::zero();
                    }
                }
            }
            row.target = target;
            row.settled = false;
        }

        // Snap-if-close fast path. If `current` is already at its
        // spec's "close enough" floor, skip the spec math: snap
        // exactly, report settled, no repaint request. This swallows
        // sub-eps drift in the caller (theme color rounded to nearest
        // ulp, etc.) that would otherwise drive a full ease/spring
        // cycle for a visually imperceptible change. The two specs use
        // *different* floors: spring tolerates pixel-scale-loose
        // residue (and checks velocity), duration uses a far tighter
        // position-only floor so a real target change always runs its
        // designed curve (see `spring.rs` for the rationale).
        let close_enough = match &row.motion {
            MotionRow::Duration { .. } => {
                within_duration_snap_eps(row.current.clone().sub(row.target.clone()))
            }
            MotionRow::Spring { velocity } => within_settle_eps(
                row.current.clone().sub(row.target.clone()),
                velocity.clone(),
            ),
        };
        if close_enough {
            row.current = row.target.clone();
            if let MotionRow::Spring { velocity } = &mut row.motion {
                *velocity = T::zero();
            }
            row.settled = true;
            return TickResult {
                current: row.target.clone(),
                settled: true,
            };
        }

        // Multi-pass guard: pass A already advanced the integrator
        // this frame. Pass B's retarget logic (above) updated `target`
        // / `segment_start` / `velocity` for the new post-action
        // state, but we don't add another dt of motion — that would
        // double the animation speed on any input frame.
        if already_advanced {
            return TickResult {
                current: row.current.clone(),
                settled: false,
            };
        }

        match spec.motion {
            AnimMotion::Duration { secs, ease } => {
                let MotionRow::Duration {
                    segment_start,
                    elapsed,
                } = &mut row.motion
                else {
                    unreachable!("motion state must match the active specification");
                };
                *elapsed += dt;
                let progress = *elapsed / secs;
                row.current = T::lerp(
                    segment_start.clone(),
                    row.target.clone(),
                    ease.apply(progress),
                );
                let settled = progress >= 1.0;
                if settled {
                    row.current = row.target.clone();
                }
                row.settled = settled;
                TickResult {
                    current: row.current.clone(),
                    settled,
                }
            }
            AnimMotion::Spring {
                stiffness,
                damping,
                substep_dt,
            } => {
                let MotionRow::Spring { velocity } = &mut row.motion else {
                    unreachable!("motion state must match the active specification");
                };
                let step = spring_step(
                    stiffness,
                    damping,
                    substep_dt,
                    row.current.clone(),
                    velocity.clone(),
                    row.target.clone(),
                    dt,
                );
                row.current = step.current;
                *velocity = step.velocity;
                row.settled = step.settled;
                TickResult {
                    current: row.current.clone(),
                    settled: step.settled,
                }
            }
        }
    }
}

impl<T: Animatable> TypedStore for AnimMapTyped<T> {
    /// Drop rows for any removed widget *and* any slot whose caller
    /// stopped poking it this frame; clear the `touched` flag on the
    /// rows that survive. Single retain pass — both predicates fold
    /// into one walk.
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
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
