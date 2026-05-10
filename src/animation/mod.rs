//! Per-`(WidgetId, AnimSlot)` animation rows, generic over
//! [`Animatable`]. See `animations.md` (next to this file) for the
//! design rationale.
//!
//! Storage is type-erased: [`AnimMap`] holds one boxed
//! [`AnimMapTyped<T>`] per `TypeId` actually used. Adding a new
//! `Animatable` type costs no central edits — first call to
//! `Ui::animate::<T>` allocates the typed slot on demand.
//! `#[derive(Animatable)]` from `palantir-anim-derive` wires the
//! math; this module wires the storage.

pub(crate) mod animatable;
pub(crate) mod easing;
pub(crate) mod spring;
#[cfg(test)]
mod tests;

use crate::animation::animatable::Animatable;
use crate::animation::easing::Easing;
use crate::animation::spring::{POS_EPS_SQ, VEL_EPS_SQ, step as spring_step};
use crate::forest::widget_id::WidgetId;
use crate::primitives::approx::approx_zero;
use rustc_hash::{FxHashMap, FxHashSet};
use std::any::{Any, TypeId};
use std::collections::hash_map::Entry;

/// Slot index for stacking multiple animations on one widget. Widgets
/// declare their own slot consts (e.g. `const HOVER: AnimSlot =
/// AnimSlot(0); const PRESS: AnimSlot = AnimSlot(1);`). Cross-widget
/// slot identity is meaningless — slot 0 on widget A is unrelated to
/// slot 0 on widget B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimSlot(pub u8);

/// How a value moves toward its target. Animation itself is opt-in
/// at the call site — pass `None` to [`crate::Ui::animate`] (or omit
/// the field on a theme) when you want snap-to-target behavior.
/// `AnimSpec` only describes what motion looks like *when there is
/// motion*; "no animation" lives in `Option<AnimSpec>`, not as a
/// variant here.
///
/// Wire format is internally tagged on `kind` (snake_case), so theme
/// files read cleanly:
///
/// ```toml
/// [theme.button.anim]
/// kind = "duration"
/// secs = 0.12
/// ease = "out_cubic"
///
/// [theme.button.anim]
/// kind = "spring"
/// stiffness = 170.0
/// damping = 26.0
/// ```
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnimSpec {
    /// Eased interpolation over `secs` seconds. `secs ≈ 0` collapses
    /// to a snap (single-frame settle).
    Duration { secs: f32, ease: Easing },
    /// Critically-damped spring (semi-implicit Euler).
    Spring { stiffness: f32, damping: f32 },
}

impl AnimSpec {
    /// 120 ms ease-out-cubic. Snappy hover/press default.
    pub const FAST: Self = Self::Duration {
        secs: 0.12,
        ease: Easing::OutCubic,
    };
    /// 200 ms ease-out-cubic. Popup reveal / panel slide default.
    pub const MEDIUM: Self = Self::Duration {
        secs: 0.2,
        ease: Easing::OutCubic,
    };
    /// Critically-damped spring tuned as a general-purpose default
    /// (Apple-style soft spring).
    pub const SPRING: Self = Self::Spring {
        stiffness: 170.0,
        damping: 26.0,
    };

    pub const fn duration(secs: f32, ease: Easing) -> Self {
        Self::Duration { secs, ease }
    }

    pub const fn spring(stiffness: f32, damping: f32) -> Self {
        Self::Spring { stiffness, damping }
    }

    /// True when this spec collapses to a single-frame snap — a
    /// `Duration` with sub-epsilon (or negative) `secs`. Springs are
    /// never instant by construction. `Ui::animate` short-circuits on
    /// this *and* on `None`, so a manually-constructed
    /// `Duration { secs: 0.0 }` behaves identically to passing `None`.
    pub fn is_instant(self) -> bool {
        match self {
            Self::Duration { secs, .. } => approx_zero(secs) || secs < 0.0,
            Self::Spring { .. } => false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AnimRow<T: Animatable> {
    pub(crate) current: T,
    pub(crate) target: T,
    pub(crate) velocity: T,      // springs only; zero for duration rows
    pub(crate) elapsed: f32,     // duration only; segment-local seconds
    pub(crate) segment_start: T, // duration only; `current` at last retarget
    /// Set by every `tick`, cleared by `end_frame`. Rows still
    /// `false` at `end_frame` are dropped — that's how a slot whose
    /// caller stopped poking it (widget id stuck around but the
    /// animation site went away) gets evicted. Without this the
    /// `(WidgetId, AnimSlot)` map only shrinks on full widget removal.
    pub(crate) touched: bool,
}

/// Per-`T` animation table. Lives inside [`AnimMap`] behind a boxed
/// trait object keyed by `TypeId`; allocated on first
/// `Ui::animate::<T>` call.
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
    ) -> TickResult<T> {
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimRow {
                    current: target,
                    target,
                    velocity: T::zero(),
                    elapsed: 0.0,
                    segment_start: target,
                    touched: true,
                });
                return TickResult {
                    current: target,
                    settled: true,
                };
            }
            Entry::Occupied(o) => o.into_mut(),
        };
        row.touched = true;

        // Retarget: duration restarts the segment from `current`;
        // spring keeps velocity (that's half the reason springs exist).
        // `Animatable: PartialEq` lets us short-circuit with a
        // bytewise compare on the steady-state path.
        if row.target != target {
            if matches!(spec, AnimSpec::Duration { .. }) {
                row.segment_start = row.current;
                row.elapsed = 0.0;
            }
            row.target = target;
        }

        // Snap-if-close fast path. If `current` is already within
        // settle epsilon of `target` and there's no residual velocity,
        // skip the spec math: snap exactly, report settled, no
        // repaint request. This swallows sub-eps drift in the caller
        // (theme color rounded to nearest ulp, etc.) that would
        // otherwise drive a full ease/spring cycle for a visually
        // imperceptible change.
        if row.current.sub(row.target).magnitude_squared() < POS_EPS_SQ
            && row.velocity.magnitude_squared() < VEL_EPS_SQ
        {
            row.current = row.target;
            row.velocity = T::zero();
            return TickResult {
                current: row.target,
                settled: true,
            };
        }

        match spec {
            AnimSpec::Duration { secs, ease } => {
                row.elapsed += dt;
                let t = (row.elapsed / secs).clamp(0.0, 1.0);
                row.current = T::lerp(row.segment_start, row.target, ease.apply(t));
                let settled = t >= 1.0;
                if settled {
                    row.current = row.target;
                }
                TickResult {
                    current: row.current,
                    settled,
                }
            }
            AnimSpec::Spring { stiffness, damping } => {
                let step = spring_step(
                    stiffness,
                    damping,
                    row.current,
                    row.velocity,
                    row.target,
                    dt,
                );
                row.current = step.current;
                row.velocity = step.velocity;
                TickResult {
                    current: row.current,
                    settled: step.settled,
                }
            }
        }
    }

    /// Drop rows for any removed widget *and* any slot whose caller
    /// stopped poking it this frame; clear the `touched` flag on the
    /// rows that survive. Single retain pass — both predicates fold
    /// into one walk.
    pub(crate) fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        self.rows.retain(|(id, _), row| {
            if removed.contains(id) {
                return false;
            }
            let kept = row.touched;
            row.touched = false;
            kept
        });
    }
}

/// Type-erased operations every typed map exposes — end-of-frame
/// sweep, plus `as_any_mut` for downcast back to the concrete map.
trait AnyTyped: 'static {
    fn end_frame(&mut self, removed: &FxHashSet<WidgetId>);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Animatable> AnyTyped for AnimMapTyped<T> {
    fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        AnimMapTyped::<T>::end_frame(self, removed);
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Central animation table on [`Ui`]. Typed maps allocated on demand
/// keyed by `TypeId`. Adding a new [`Animatable`] type costs no
/// central edits — first `Ui::animate::<T>` call boxes a fresh
/// `AnimMapTyped<T>`.
#[derive(Default)]
pub(crate) struct AnimMap {
    by_type: FxHashMap<TypeId, Box<dyn AnyTyped>>,
}

impl AnimMap {
    /// Get-or-create the typed map for `T`. Allocates on first call
    /// per `T`; subsequent calls hit the hashmap and downcast.
    pub(crate) fn typed_mut<T: Animatable>(&mut self) -> &mut AnimMapTyped<T> {
        self.by_type
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::<AnimMapTyped<T>>::default())
            .as_any_mut()
            .downcast_mut::<AnimMapTyped<T>>()
            .expect("TypeId is stable per T, downcast cannot fail")
    }

    /// Borrow the typed map for `T` if it exists. Used by the
    /// `Ui::animate(.., None)` short-circuit to drop a stale row
    /// without allocating a fresh typed map.
    pub(crate) fn try_typed_mut<T: Animatable>(&mut self) -> Option<&mut AnimMapTyped<T>> {
        self.by_type
            .get_mut(&TypeId::of::<T>())?
            .as_any_mut()
            .downcast_mut::<AnimMapTyped<T>>()
    }

    /// Drop rows for removed widgets and for slots that weren't
    /// poked this frame, then clear the `touched` flags on the rows
    /// that survive. Called from `Ui::end_frame` once per frame; the
    /// `removed` set is the same one that drives `StateMap` / text /
    /// layout sweeps. A `(WidgetId, AnimSlot)` row goes away if
    /// either (a) the widget itself disappeared or (b) the call site
    /// that owns the slot stopped reaching for it — without (b),
    /// abandoned slots would accumulate forever for any widget
    /// whose id lingers across motion-toggle states.
    pub(crate) fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        if self.by_type.is_empty() {
            return;
        }
        for typed in self.by_type.values_mut() {
            typed.end_frame(removed);
        }
    }
}
