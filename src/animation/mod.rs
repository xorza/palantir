//! Per-`(WidgetId, AnimSlot)` animation rows, generic over
//! [`Animatable`]. See `docs/animations.md` for the design rationale.
//!
//! Storage is per-type (one [`AnimMapTyped`] field on [`AnimMap`] per
//! supported `T`) so the hot path stays type-erasure-free. Adding a
//! new `T` = implement `Animatable` + add a typed slot here.

pub(crate) mod animatable;
pub(crate) mod easing;
pub(crate) mod spring;
#[cfg(test)]
mod tests;

use crate::animation::animatable::Animatable;
use crate::animation::easing::Easing;
use crate::primitives::approx::approx_zero;
use crate::primitives::color::Color;
use crate::tree::widget_id::WidgetId;
use glam::Vec2;
use rustc_hash::FxHashMap;
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
    pub(crate) spec: AnimSpec,
}

/// Per-`T` animation table. Public so it can appear in
/// [`Animatable::slot_mut`]'s signature, but opaque — fields and
/// methods are crate-internal.
pub struct AnimMapTyped<T: Animatable> {
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
    /// `AnimSpec::INSTANT` (and any `Duration { secs <= 0, .. }`)
    /// short-circuits: snap to target, drop any stored row, return
    /// settled. No allocation, no repaint request — using INSTANT is
    /// indistinguishable from not calling `animate` at all.
    pub(crate) fn tick(
        &mut self,
        id: WidgetId,
        slot: AnimSlot,
        target: T,
        spec: AnimSpec,
        dt: f32,
    ) -> TickResult<T> {
        if spec.is_instant() {
            // Drop any stale row so `current` doesn't carry over if
            // the caller switches back to a real spec.
            self.rows.remove(&(id, slot));
            return TickResult {
                current: target,
                settled: true,
            };
        }

        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimRow {
                    current: target,
                    target,
                    velocity: T::zero(),
                    elapsed: 0.0,
                    segment_start: target,
                    spec,
                });
                return TickResult {
                    current: target,
                    settled: true,
                };
            }
            Entry::Occupied(o) => o.into_mut(),
        };

        row.spec = spec;

        // Retarget: duration restarts the segment from `current`;
        // spring keeps velocity (that's half the reason springs exist).
        if row.target.sub(target).magnitude() > 0.0 {
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
        if row.current.sub(row.target).magnitude() < crate::animation::spring::POS_EPS
            && row.velocity.magnitude() < crate::animation::spring::VEL_EPS
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
                let t = (row.elapsed / secs.max(f32::EPSILON)).clamp(0.0, 1.0);
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
                let step = crate::animation::spring::step(
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

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        if removed.is_empty() {
            return;
        }
        self.rows.retain(|(id, _), _| !removed.contains(id));
    }
}

/// Central animation table on [`Ui`]. One typed slot per supported
/// `T`. Sweep fans out across all slots. Public so it can appear in
/// [`Animatable::slot_mut`]'s signature, but opaque — fields and
/// methods are crate-internal.
#[derive(Default)]
pub struct AnimMap {
    pub(crate) scalars: AnimMapTyped<f32>,
    pub(crate) vec2s: AnimMapTyped<Vec2>,
    pub(crate) colors: AnimMapTyped<Color>,
}

impl AnimMap {
    /// Drop every row (across all typed slots) belonging to a removed
    /// widget. Called from `Ui::end_frame` with the same `removed`
    /// slice that drives `StateMap` / text / layout sweeps.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        if removed.is_empty() {
            return;
        }
        self.scalars.sweep_removed(removed);
        self.vec2s.sweep_removed(removed);
        self.colors.sweep_removed(removed);
    }
}
