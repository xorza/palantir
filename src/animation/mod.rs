//! Per-`(WidgetId, AnimSlot)` animation rows, generic over
//! [`Animatable`]. See `animations.md` (next to this file) for the
//! design rationale.
//!
//! Storage is type-erased: [`AnimMap`] holds one boxed
//! [`AnimMapTyped<T>`] per `TypeId` actually used. Adding a new
//! `Animatable` type costs no central edits — first call to
//! `Ui::animate::<T>` allocates the typed slot on demand.
//! `#[derive(Animatable)]` from `aperture-anim-derive` wires the
//! math; this module wires the storage.

pub(crate) mod animatable;
pub(crate) mod easing;
pub(crate) mod spring;

use crate::animation::animatable::Animatable;
use crate::animation::easing::Easing;
use crate::animation::spring::{step as spring_step, within_duration_snap_eps, within_settle_eps};
use crate::primitives::approx::approx_zero;
use crate::primitives::widget_id::WidgetId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::any::{Any, TypeId};
use std::collections::hash_map::Entry;

/// Slot tag for stacking multiple animations on one widget. Widgets
/// declare their own slot consts (e.g. `const HOVER: AnimSlot =
/// AnimSlot("hover"); const PRESS: AnimSlot = AnimSlot("press");`).
/// Cross-widget slot identity is meaningless — `AnimSlot("hover")` on
/// widget A is unrelated to `AnimSlot("hover")` on widget B (the
/// hash key is `(WidgetId, AnimSlot)`).
///
/// Stored as `&'static str` so the slot reads as a name at the call
/// site instead of a magic number; equality / hashing is by string
/// *contents* (std's `str` impls compare length-then-bytes and hash
/// the bytes — no pointer fast-path), so the same literal from
/// multiple call sites compares equal regardless of interning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimSlot(pub &'static str);

impl From<&'static str> for AnimSlot {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self(s)
    }
}

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
    /// Set by every `tick`, cleared by `post_record`. Rows still
    /// `false` at `post_record` are dropped — that's how a slot whose
    /// caller stopped poking it (widget id stuck around but the
    /// animation site went away) gets evicted. Without this the
    /// `(WidgetId, AnimSlot)` map only shrinks on full widget removal.
    pub(crate) touched: bool,
    /// `Ui::frame_id` at the last `tick` that ran the integrator
    /// step. A second `tick` in the same frame (multi-pass record:
    /// `run_frame` re-runs `build` after an input action drains) sees
    /// this match and short-circuits the dt-driven advance, so the
    /// integrator advances exactly once per host frame. Retarget
    /// logic still runs in the short-circuited call so pass B's
    /// post-action target replaces pass A's stale one.
    pub(crate) advanced_at: u64,
    /// Cached settle state, set true on insert / when the integrator
    /// or `within_settle_eps` confirms settlement, false on retarget.
    /// Lets `tick` fast-return on a steady-state row without the
    /// `sub` + `magnitude_squared` settle math; the `PartialEq`
    /// retarget compare still runs so a target change unfreezes the
    /// row immediately.
    pub(crate) settled: bool,
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
        frame_id: u64,
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
                    velocity: T::zero(),
                    elapsed: 0.0,
                    segment_start: target.clone(),
                    touched: true,
                    advanced_at: frame_id,
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
        let already_advanced = row.advanced_at == frame_id;
        row.advanced_at = frame_id;

        // Steady-state fast path. Once a row settles, every subsequent
        // tick with the same target should be a no-op — skip the
        // `sub` + `magnitude_squared` settle math entirely. Retarget
        // detection still runs (the `target != row.target` compare
        // below) so a caller changing the target unfreezes the row
        // immediately.
        if row.settled && row.target == target {
            return TickResult {
                current: row.current.clone(),
                settled: true,
            };
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
            match spec {
                AnimSpec::Duration { .. } => {
                    row.segment_start = row.current.clone();
                    row.elapsed = 0.0;
                    // Zero residual spring velocity so a Spring →
                    // Duration switch starts the new segment from
                    // rest. Without this, the snap-if-close check
                    // below could falsely fail and the lerp would
                    // compose with leftover spring motion that has no
                    // place in a duration animation.
                    row.velocity = T::zero();
                }
                AnimSpec::Spring { .. } => {
                    let to_target = target.clone().sub(row.current.clone());
                    if dot(row.velocity.clone(), to_target) < 0.0 {
                        row.velocity = T::zero();
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
        let close_enough = match spec {
            AnimSpec::Duration { .. } => {
                within_duration_snap_eps(row.current.clone().sub(row.target.clone()))
            }
            AnimSpec::Spring { .. } => within_settle_eps(
                row.current.clone().sub(row.target.clone()),
                row.velocity.clone(),
            ),
        };
        if close_enough {
            row.current = row.target.clone();
            row.velocity = T::zero();
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

        match spec {
            AnimSpec::Duration { secs, ease } => {
                row.elapsed += dt;
                let progress = row.elapsed / secs;
                row.current = T::lerp(
                    row.segment_start.clone(),
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
            AnimSpec::Spring { stiffness, damping } => {
                let step = spring_step(
                    stiffness,
                    damping,
                    row.current.clone(),
                    row.velocity.clone(),
                    row.target.clone(),
                    dt,
                );
                row.current = step.current;
                row.velocity = step.velocity;
                row.settled = step.settled;
                TickResult {
                    current: row.current.clone(),
                    settled: step.settled,
                }
            }
        }
    }

    /// Drop rows for any removed widget *and* any slot whose caller
    /// stopped poking it this frame; clear the `touched` flag on the
    /// rows that survive. Single retain pass — both predicates fold
    /// into one walk.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
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
/// sweep plus an emptiness probe so the parent can drop drained maps.
/// `: Any` is what lets the downcast sites upcast a `&mut dyn
/// AnyTyped` straight to `&mut dyn Any` — no `as_any` boilerplate.
pub(crate) trait AnyTyped: Any {
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>);
    fn is_empty(&self) -> bool;
}

impl<T: Animatable> AnyTyped for AnimMapTyped<T> {
    fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        AnimMapTyped::<T>::sweep_removed(self, removed);
    }
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Central animation table on [`Ui`]. Typed maps allocated on demand
/// keyed by `TypeId`. Adding a new [`Animatable`] type costs no
/// central edits — first `Ui::animate::<T>` call boxes a fresh
/// `AnimMapTyped<T>`.
#[derive(Default)]
pub(crate) struct AnimMap {
    pub(crate) by_type: FxHashMap<TypeId, Box<dyn AnyTyped>>,
}

impl AnimMap {
    /// Get-or-create the typed map for `T`. Allocates on first call
    /// per `T`; subsequent calls hit the hashmap and downcast.
    pub(crate) fn typed_mut<T: Animatable>(&mut self) -> &mut AnimMapTyped<T> {
        (self
            .by_type
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::<AnimMapTyped<T>>::default())
            .as_mut() as &mut dyn Any)
            .downcast_mut::<AnimMapTyped<T>>()
            .expect("TypeId is stable per T, downcast cannot fail")
    }

    /// Borrow the typed map for `T` if it exists. Used by the
    /// `Ui::animate(.., None)` short-circuit to drop a stale row
    /// without allocating a fresh typed map.
    pub(crate) fn try_typed_mut<T: Animatable>(&mut self) -> Option<&mut AnimMapTyped<T>> {
        (self.by_type.get_mut(&TypeId::of::<T>())?.as_mut() as &mut dyn Any)
            .downcast_mut::<AnimMapTyped<T>>()
    }

    /// Drop rows for removed widgets and for slots that weren't
    /// poked this frame, then clear the `touched` flags on the rows
    /// that survive. Called from `Ui::finalize_frame` once per frame; the
    /// `removed` set is the same one that drives `StateMap` / text /
    /// layout sweeps. A `(WidgetId, AnimSlot)` row goes away if
    /// either (a) the widget itself disappeared or (b) the call site
    /// that owns the slot stopped reaching for it — without (b),
    /// abandoned slots would accumulate forever for any widget
    /// whose id lingers across motion-toggle states.
    ///
    /// A typed map that drains to empty is dropped entirely: it's
    /// re-created lazily on the next `typed_mut::<T>`, and keeping it
    /// would leave `by_type` non-empty forever, permanently disabling
    /// the `by_type.is_empty()` fast path in `Ui::animate` once *any*
    /// widget has ever animated — even after the app goes idle.
    pub(crate) fn sweep_removed(&mut self, removed: &FxHashSet<WidgetId>) {
        if self.by_type.is_empty() {
            return;
        }
        self.by_type.retain(|_, typed| {
            typed.sweep_removed(removed);
            !typed.is_empty()
        });
    }
}

#[cfg(test)]
mod tests;
