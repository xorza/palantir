//! Paint-only animations: the per-shape contract (`PaintAnim` /
//! `PaintMod`) and the per-tree registry that stores it.
//!
//! Paint anims are declarative shape-level animations that don't affect
//! layout, hit-test, or tree structure. Widgets register a `PaintAnim`
//! against a freshly-added shape via `Ui::add_shape_animated`; the
//! encoder samples it at paint time and folds the resulting [`PaintMod`]
//! (an alpha multiplier today; transform mod once the renderer can
//! express it) into the per-shape brush. `post_record` folds each anim's
//! `next_wake` into the `Ui` frame runtime's wake queue, so widget code never calls
//! `request_repaint_after` for these shapes.
//!
//! Unlike the value-interpolation animations in `crate::animation`
//! (record-time readback, keyed `(WidgetId, AnimSlot)`), paint anims are
//! sampled at *encode* time and stored on the `Tree`. They share no code
//! with that system — sampling is a pure function of `now`, with no
//! accumulator state, so dropped frames / irregular `dt` don't drift.
//!
//! The registry pairs the list of live entries with a shape-indexed
//! lookup table so the encoder can map `(shape_idx) → PaintMod` in one
//! indexed load + branch on the hot path. `by_shape` is **lazy**: empty
//! when no shape this frame is animated, and only grown out to
//! `shape_idx + 1` on the first `push_entry` call. Encoder treats
//! `shape_idx >= by_shape.len()` as "no anim" so the no-anim path costs
//! one length compare, and `Forest::add_shape` doesn't push a sentinel
//! per shape in the common (no-anim) frame.
//!
//! [`PaintAnim::BlinkOpacity`] (alpha) and [`PaintAnim::Spin`] (rotation)
//! ship today; pulse or marquee variants would need further encoder
//! transform-mod plumbing.

use crate::primitives::approx::approx_zero;
use std::f32::consts::TAU;
use std::time::Duration;

/// A paint-time animation contract. Encoded as a small enum so the
/// per-shape registry stays a flat `Vec`; sampling is branch-on-tag
/// rather than a virtual call.
///
/// Sampling is a pure function of `now`. No accumulator state, so
/// dropped frames / irregular `dt` don't drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PaintAnim {
    /// Solid for `half_period`, hidden for the next `half_period`,
    /// repeating from `started_at`. The caret-blink shape.
    BlinkOpacity {
        half_period: Duration,
        started_at: Duration,
    },
    /// Continuous rotation at `speed` radians/second, measured from
    /// `started_at`. The sampled angle is `(now - started_at) * speed`
    /// wrapped to `[0, TAU)`. Its [`Self::next_wake`] is always `now`, so
    /// it repaints every frame (a spinner) without the widget changing
    /// any geometry — the arc is recorded once and spun at paint time.
    Spin { speed: f32, started_at: Duration },
}

/// Per-shape paint modification sampled from a `PaintAnim`. Encoder
/// folds this into the shape's brush / geometry at emit time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMod {
    /// Multiplies the shape's fill alpha. `1.0` = pass-through;
    /// `0.0` = fully transparent (encoder may drop the emit).
    pub(crate) alpha: f32,
    /// Rotation in radians applied to the shape's geometry about its
    /// owner-box centre at paint time. `0.0` = no rotation. Only
    /// [`PaintAnim::Spin`] produces a non-zero value today; the
    /// polyline, curve, and arc emits honour it (the composer rotates
    /// points / control points / center + angles before the ancestor
    /// transform — see the encoder's `spin_bbox` pivot contract).
    pub(crate) rotation: f32,
}

impl PaintMod {
    /// Pass-through sample. Returned by [`PaintAnims::sample`] when a
    /// shape has no anim attached, so callers can fold the result
    /// unconditionally.
    pub(crate) const IDENTITY: Self = Self {
        alpha: 1.0,
        rotation: 0.0,
    };

    #[allow(dead_code)] // consumed once Pulse/Marquee land
    #[inline]
    pub(crate) fn is_identity(&self) -> bool {
        // Within `approx::EPS` of 1.0, not `>= 1.0`: a sampled alpha of
        // 0.9999 is a visually exact pass-through, while over-bright
        // (`alpha > 1 + EPS`) is correctly *not* identity. Today's only
        // sampler emits {0.0, 1.0}, but the predicate must stay honest
        // for the non-binary variants it's reserved for.
        approx_zero(self.alpha - 1.0) && approx_zero(self.rotation)
    }
}

impl PaintAnim {
    /// Sample the animation at `now`. Pure function — caller is
    /// responsible for clamping `now >= started_at` (this routine
    /// tolerates `now < started_at` by returning the pre-start
    /// phase's value).
    #[inline]
    pub(crate) fn sample(self, now: Duration) -> PaintMod {
        match self {
            PaintAnim::BlinkOpacity {
                half_period,
                started_at,
            } => {
                let alpha = if blink_visible_at(half_period, started_at, now) {
                    1.0
                } else {
                    0.0
                };
                PaintMod {
                    alpha,
                    rotation: 0.0,
                }
            }
            PaintAnim::Spin { speed, started_at } => {
                // Wrap to `[0, TAU)` so `sin_cos` keeps full precision no
                // matter how long the spinner has been on screen.
                let dt = now.saturating_sub(started_at).as_secs_f32();
                let rotation = (dt * speed).rem_euclid(TAU);
                PaintMod {
                    alpha: 1.0,
                    rotation,
                }
            }
        }
    }

    /// Earliest `Duration` (absolute time, same epoch as
    /// frame-runtime time / `started_at`) at which `quantum` will next
    /// change. `post_record` folds the min of every live entry's
    /// `next_wake` into the frame runtime's wake queue so widgets don't have to.
    ///
    /// For `BlinkOpacity` this is the next half-period boundary
    /// strictly after `now`.
    #[inline]
    pub(crate) fn next_wake(self, now: Duration) -> Duration {
        match self {
            PaintAnim::BlinkOpacity {
                half_period,
                started_at,
            } => next_blink_boundary(half_period, started_at, now),
            // Continuous: the angle changes every frame, so the soonest
            // it "next changes" is now. `extend_predamaged` compares
            // `next_wake(prev) <= now` (always true, since `prev <= now`)
            // and so re-damages the spun shape's rect every frame.
            PaintAnim::Spin { .. } => now,
        }
    }
}

/// True when a blink with `half_period` starting at `started_at` is
/// in its solid phase at `now`. Pre-start (now < started_at) returns
/// `true` so a freshly-focused caret is immediately visible.
#[inline]
fn blink_visible_at(half_period: Duration, started_at: Duration, now: Duration) -> bool {
    if now <= started_at {
        return true;
    }
    let dt = now - started_at;
    // (dt / half_period) parity: even = solid, odd = hidden.
    let n = duration_div_floor(dt, half_period);
    n & 1 == 0
}

/// Absolute time of the next strictly-future boundary at which the
/// blink flips. Aligns to `started_at + k * half_period` for the
/// smallest `k` with that time `> now`.
#[inline]
fn next_blink_boundary(half_period: Duration, started_at: Duration, now: Duration) -> Duration {
    if half_period.is_zero() {
        return Duration::MAX;
    }
    if now < started_at {
        return started_at;
    }
    let dt = now - started_at;
    let n = duration_div_floor(dt, half_period);
    started_at + half_period.saturating_mul((n + 1) as u32)
}

/// `floor(a / b)` for `Duration`. Returns 0 if `b` is zero.
#[inline]
fn duration_div_floor(a: Duration, b: Duration) -> u64 {
    let bn = b.as_nanos();
    if bn == 0 {
        return 0;
    }
    (a.as_nanos() / bn) as u64
}

/// Sentinel in [`PaintAnims::by_shape`] meaning "this shape has no
/// paint-anim registration". `u16::MAX` mirrors the niche convention
/// used by `Slot::ABSENT` for the sparse extras tables — keeps the
/// encoder's per-shape lookup a single load + cmp.
const PAINT_ANIM_NONE: u16 = u16::MAX;

/// One row per registered paint animation. Lives in
/// [`PaintAnims::entries`], indexed by `by_shape[shape_idx]` (which
/// holds [`PAINT_ANIM_NONE`] when the shape isn't animated).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintAnimEntry {
    pub(crate) anim: PaintAnim,
    /// Paint-arena row of the animated shape inside its owner's
    /// `node_spans` span — the chrome offset plus the shape's position
    /// in the owner's `TreeItems` stream, captured from
    /// `OpenFrame::paint_rows` at `add_shape_animated` time. Lets
    /// damage's `extend_predamaged` index the shape's screen rect as
    /// `paint_arena.rows[node_span.start + row]` with no per-frame
    /// `TreeItems` walk.
    pub(crate) row: u32,
    /// Index into `Tree::records` of the node that owns this shape —
    /// the open node at `add_shape_animated` time. Lets the damage
    /// lookup index `node_spans[node_idx]` directly without needing a
    /// per-frame `shape_idx → paint_idx` reverse map.
    pub(crate) node_idx: u32,
}

/// Per-tree paint-animation registry. Pushed in lockstep with the
/// shape buffer; cleared per frame.
#[derive(Debug, Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order. Iterated by
    /// `Forest::min_paint_anim_wake` (next-wake fold) and
    /// `DamageEngine::compute` (anim-damage union).
    pub(crate) entries: Vec<PaintAnimEntry>,
    /// Sparse `shape_idx → entries[idx]` lookup. Empty when no shape
    /// this frame is animated (the common case). Grown only when the
    /// first animated shape arrives — padded out to `shape_idx + 1`
    /// with [`PAINT_ANIM_NONE`] and the animated slot stamped. Encoder
    /// treats `shape_idx >= by_shape.len()` as "no anim", so unanimated
    /// shapes pay zero `Vec::push` per frame.
    ///
    /// Capacity caps animated shapes per tree at `u16::MAX - 1`,
    /// which is well past anything realistic (caret, spinner, pulse
    /// — order of dozens, not thousands).
    pub(crate) by_shape: Vec<u16>,
}

impl PaintAnims {
    /// Reset both columns for a fresh recording frame. Capacity
    /// retained — same lifecycle as every other per-frame tree
    /// column.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_shape.clear();
    }

    /// Register `entry` against the just-pushed shape at `shape_idx`
    /// (its index into `Tree::shapes.records`). Lazily grows
    /// `by_shape` to `shape_idx + 1`, padding any preceding
    /// (unanimated) shapes with [`PAINT_ANIM_NONE`]. Asserts the
    /// `entries` cap so a `u16` index always fits in `by_shape`.
    pub(crate) fn push_entry(&mut self, shape_idx: u32, entry: PaintAnimEntry) {
        let idx = self.entries.len();
        debug_assert!(
            idx < PAINT_ANIM_NONE as usize,
            "more than {PAINT_ANIM_NONE} paint-anim entries in one tree — bump by_shape to u32",
        );
        let shape_idx = shape_idx as usize;
        if self.by_shape.len() <= shape_idx {
            self.by_shape.resize(shape_idx + 1, PAINT_ANIM_NONE);
        }
        self.by_shape[shape_idx] = idx as u16;
        self.entries.push(entry);
    }

    /// Sample the anim attached to shape `shape_idx`, if any. Returns
    /// [`PaintMod::IDENTITY`] on the hot path (no anim — the vast
    /// majority of shapes), so callers can fold the result
    /// unconditionally once we ship variants beyond binary blink.
    #[inline]
    pub(crate) fn sample(&self, shape_idx: u32, now: Duration) -> PaintMod {
        let Some(&slot) = self.by_shape.get(shape_idx as usize) else {
            return PaintMod::IDENTITY;
        };
        if slot == PAINT_ANIM_NONE {
            return PaintMod::IDENTITY;
        }
        self.entries[slot as usize].anim.sample(now)
    }
}

#[cfg(test)]
mod tests {
    use crate::forest::tree::paint_anims::*;

    const HP: Duration = Duration::from_millis(500);
    const START: Duration = Duration::from_secs(1);

    #[test]
    fn blink_solid_at_start() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        assert_eq!(a.sample(START).alpha, 1.0);
    }

    #[test]
    fn blink_flips_at_first_boundary() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        // Just before the boundary: still solid.
        let before = START + HP - Duration::from_micros(1);
        assert_eq!(a.sample(before).alpha, 1.0);
        // At the boundary: hidden.
        let at = START + HP;
        assert_eq!(a.sample(at).alpha, 0.0);
        // Two boundaries later: solid again.
        let two = START + HP + HP;
        assert_eq!(a.sample(two).alpha, 1.0);
    }

    #[test]
    fn next_wake_aligns_with_next_boundary() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        // Mid-phase: wake at the next half-period boundary.
        assert_eq!(a.next_wake(START + Duration::from_millis(100)), START + HP,);
        // On the boundary: still wake at the *next* one (strictly
        // future).
        assert_eq!(a.next_wake(START + HP), START + HP + HP);
        // Several periods in.
        assert_eq!(
            a.next_wake(START + HP + HP + Duration::from_millis(50)),
            START + HP + HP + HP,
        );
    }

    #[test]
    fn pre_start_phase_is_solid_and_wakes_at_start() {
        let a = PaintAnim::BlinkOpacity {
            half_period: HP,
            started_at: START,
        };
        let before = START - Duration::from_millis(200);
        assert_eq!(a.sample(before).alpha, 1.0);
        assert_eq!(a.next_wake(before), START);
    }

    #[test]
    fn zero_period_never_wakes() {
        let a = PaintAnim::BlinkOpacity {
            half_period: Duration::ZERO,
            started_at: START,
        };
        // Degenerate, but must not panic. `next_wake` returns MAX so
        // the wake folder treats it as "idle".
        assert_eq!(a.next_wake(START + Duration::from_secs(1)), Duration::MAX);
    }

    #[test]
    fn paint_mod_identity_is_pass_through() {
        assert!(PaintMod::IDENTITY.is_identity());
        assert!(
            PaintMod {
                alpha: 1.0,
                rotation: 0.0
            }
            .is_identity()
        );
        assert!(
            !PaintMod {
                alpha: 0.5,
                rotation: 0.0
            }
            .is_identity()
        );
        // A non-zero rotation is not a pass-through even at full alpha.
        assert!(
            !PaintMod {
                alpha: 1.0,
                rotation: 0.5
            }
            .is_identity()
        );
    }

    #[test]
    fn spin_angle_is_elapsed_times_speed_wrapped() {
        let speed = 4.0; // rad/s
        let a = PaintAnim::Spin {
            speed,
            started_at: START,
        };
        // Pre-start clamps to 0 (no negative elapsed).
        assert_eq!(a.sample(START - Duration::from_secs(1)).rotation, 0.0);
        // 0.25 s in → 1.0 rad, alpha untouched.
        let m = a.sample(START + Duration::from_millis(250));
        assert!((m.rotation - 1.0).abs() < 1e-5, "rotation {}", m.rotation);
        assert_eq!(m.alpha, 1.0);
        // 2 s in → 8.0 rad, wrapped into [0, TAU): 8 - TAU ≈ 1.7168.
        let wrapped = a.sample(START + Duration::from_secs(2)).rotation;
        let expect = 8.0_f32.rem_euclid(TAU);
        assert!((wrapped - expect).abs() < 1e-4, "wrapped {wrapped}");
        assert!((0.0..TAU).contains(&wrapped));
    }

    #[test]
    fn spin_wakes_every_frame() {
        // `next_wake(prev)` must be <= now for any prev <= now so
        // `extend_predamaged` repaints the spun rect each frame.
        let a = PaintAnim::Spin {
            speed: 1.0,
            started_at: START,
        };
        let prev = START + Duration::from_secs(3);
        let now = prev + Duration::from_millis(16);
        assert!(a.next_wake(prev) <= now);
    }
}
