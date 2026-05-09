//! Per-`(WidgetId, AnimSlot)` animation rows. See `docs/animations.md`
//! for the design rationale; this module is the f32 implementation
//! (Phase 2). Generic `T: Lerp` lands in Phase 3.

pub(crate) mod easing;
pub(crate) mod spring;
#[cfg(test)]
mod tests;

use crate::animation::easing::Easing;
use crate::animation::spring::Spring;
use crate::tree::widget_id::WidgetId;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Slot index for stacking multiple animations on one widget. Widgets
/// declare their own slot consts (e.g. `const HOVER: AnimSlot =
/// AnimSlot(0); const PRESS: AnimSlot = AnimSlot(1);`). Cross-widget
/// slot identity is meaningless — slot 0 on widget A is unrelated to
/// slot 0 on widget B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimSlot(pub u8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimSpec {
    Duration { secs: f32, ease: Easing },
    Spring(Spring),
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
    pub const SPRING: Self = Self::Spring(Spring {
        stiffness: 170.0,
        damping: 26.0,
    });

    pub const fn duration(secs: f32, ease: Easing) -> Self {
        Self::Duration { secs, ease }
    }

    pub const fn spring(stiffness: f32, damping: f32) -> Self {
        Self::Spring(Spring { stiffness, damping })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AnimRowF32 {
    pub(crate) current: f32,
    pub(crate) target: f32,
    pub(crate) velocity: f32,      // springs only; 0.0 for duration rows
    pub(crate) elapsed: f32,       // duration only; segment-local seconds
    pub(crate) segment_start: f32, // duration only; `current` at last retarget
    pub(crate) spec: AnimSpec,
}

#[derive(Default)]
pub(crate) struct AnimMap {
    pub(crate) rows: FxHashMap<(WidgetId, AnimSlot), AnimRowF32>,
}

pub(crate) struct TickResult {
    pub(crate) current: f32,
    pub(crate) settled: bool,
}

impl AnimMap {
    /// Insert-or-advance. First touch snaps `current = target` and
    /// returns settled — there's no animation on appearance, by
    /// design. Subsequent calls detect retarget vs steady-state and
    /// advance the row by `dt` seconds.
    pub(crate) fn tick_f32(
        &mut self,
        id: WidgetId,
        slot: AnimSlot,
        target: f32,
        spec: AnimSpec,
        dt: f32,
    ) -> TickResult {
        let row = match self.rows.entry((id, slot)) {
            Entry::Vacant(v) => {
                v.insert(AnimRowF32 {
                    current: target,
                    target,
                    velocity: 0.0,
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
        if row.target != target {
            if matches!(spec, AnimSpec::Duration { .. }) {
                row.segment_start = row.current;
                row.elapsed = 0.0;
            }
            row.target = target;
        }

        match spec {
            AnimSpec::Duration { secs, ease } => {
                row.elapsed += dt;
                let t = (row.elapsed / secs.max(f32::EPSILON)).clamp(0.0, 1.0);
                row.current = lerp(row.segment_start, row.target, ease.apply(t));
                let settled = t >= 1.0;
                if settled {
                    row.current = row.target;
                }
                TickResult {
                    current: row.current,
                    settled,
                }
            }
            AnimSpec::Spring(spring) => {
                let step = spring.step(row.current, row.velocity, row.target, dt);
                row.current = step.current;
                row.velocity = step.velocity;
                TickResult {
                    current: row.current,
                    settled: step.settled,
                }
            }
        }
    }

    /// Drop every slot belonging to a removed widget. Called from
    /// `Ui::end_frame` with the same `removed` slice that drives
    /// `StateMap` / text / layout sweeps. O(rows × removed) — fine
    /// while widget counts and removal counts are both small; if
    /// either grows substantially, switch to a two-level map keyed on
    /// `WidgetId` first.
    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        if removed.is_empty() {
            return;
        }
        self.rows.retain(|(id, _), _| !removed.contains(id));
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
