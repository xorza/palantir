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
//! The registry stores only live entries and their sorted shape indices.
//! Encoder traversal is monotonic in shape index, so a cursor advances
//! across both visited shapes and ranges skipped by subtree culling without
//! retaining a reverse-index slot for every preceding static shape.
//!
//! [`PaintAnim::BlinkOpacity`] (alpha) and [`PaintAnim::Spin`] (rotation)
//! ship today; pulse or marquee variants would need further encoder
//! transform-mod plumbing.

use std::f32::consts::TAU;
use std::time::Duration;

const CURSOR_END: u64 = u64::MAX;

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
    /// Pass-through sample. Returned by [`PaintAnimCursor::sample`] when a
    /// shape has no anim attached, so callers can fold the result
    /// unconditionally.
    pub(crate) const IDENTITY: Self = Self {
        alpha: 1.0,
        rotation: 0.0,
    };
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

/// One row per registered paint animation.
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

/// Per-tree sparse paint-animation registry, cleared per frame.
#[derive(Debug, Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order. Iterated by
    /// `Forest::min_paint_anim_wake` (next-wake fold) and
    /// `DamageEngine::compute` (anim-damage union).
    pub(crate) entries: Vec<PaintAnimEntry>,
    /// Shape indices parallel to `entries`, strictly increasing because
    /// registration follows append-only shape recording.
    pub(crate) shape_indices: Vec<u32>,
}

impl PaintAnims {
    /// Reset both columns for a fresh recording frame. Capacity
    /// retained — same lifecycle as every other per-frame tree
    /// column.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.shape_indices.clear();
    }

    /// Register `entry` against the just-pushed shape at `shape_idx`
    /// (its index into `Tree::shapes.records`).
    pub(crate) fn push_entry(&mut self, shape_idx: u32, entry: PaintAnimEntry) {
        assert!(
            self.shape_indices
                .last()
                .is_none_or(|&last| last < shape_idx),
            "paint animation shape indices must be strictly increasing",
        );
        self.shape_indices.push(shape_idx);
        self.entries.push(entry);
    }

    pub(crate) fn cursor(&self) -> PaintAnimCursor<'_> {
        PaintAnimCursor {
            shape_indices: &self.shape_indices,
            entries: &self.entries,
            next: 0,
            next_shape: self
                .shape_indices
                .first()
                .map_or(CURSOR_END, |&shape_idx| shape_idx as u64),
        }
    }
}

/// Monotonic encoder lookup over the sparse animation rows.
#[derive(Debug)]
pub(crate) struct PaintAnimCursor<'a> {
    shape_indices: &'a [u32],
    entries: &'a [PaintAnimEntry],
    next: usize,
    next_shape: u64,
}

impl PaintAnimCursor<'_> {
    /// `shape_idx` must increase between calls. Jumps are allowed because
    /// viewport and damage culling can skip whole shape ranges.
    #[inline]
    pub(crate) fn sample(&mut self, shape_idx: u32, now: Duration) -> PaintMod {
        let shape_idx = shape_idx as u64;
        if shape_idx < self.next_shape {
            return PaintMod::IDENTITY;
        }
        while shape_idx > self.next_shape {
            self.advance();
        }
        if self.next_shape == CURSOR_END {
            return PaintMod::IDENTITY;
        }
        let entry = self.entries[self.next];
        self.advance();
        entry.anim.sample(now)
    }

    #[inline]
    fn advance(&mut self) {
        self.next += 1;
        self.next_shape = self
            .shape_indices
            .get(self.next)
            .map_or(CURSOR_END, |&shape_idx| shape_idx as u64);
    }
}

#[cfg(test)]
mod tests {
    use crate::scene::tree::paint_anims::*;

    const HP: Duration = Duration::from_millis(500);
    const START: Duration = Duration::from_secs(1);

    fn spinning(speed: f32) -> PaintAnimEntry {
        PaintAnimEntry {
            anim: PaintAnim::Spin {
                speed,
                started_at: START,
            },
            row: 0,
            node_idx: 0,
        }
    }

    #[test]
    fn sparse_cursor_samples_boundaries_and_advances_across_skipped_animations() {
        const LAST_SHAPE: u32 = 1_000_000;
        let mut anims = PaintAnims::default();
        anims.push_entry(0, spinning(1.0));
        anims.push_entry(5, spinning(2.0));
        anims.push_entry(10, spinning(3.0));
        anims.push_entry(LAST_SHAPE, spinning(4.0));

        assert_eq!(anims.shape_indices, [0, 5, 10, LAST_SHAPE]);
        assert_eq!(anims.entries.len(), 4);
        assert_eq!(
            std::mem::size_of_val(anims.shape_indices.as_slice()),
            4 * std::mem::size_of::<u32>(),
        );

        let now = START + Duration::from_secs(1);
        let mut cursor = anims.cursor();
        assert_eq!(cursor.sample(0, now).rotation, 1.0);
        assert_eq!(cursor.sample(1, now), PaintMod::IDENTITY);
        assert_eq!(cursor.sample(5, now).rotation, 2.0);
        assert_eq!(
            cursor.sample(LAST_SHAPE, now).rotation,
            4.0,
            "jumping over culled shape 10 must not strand the cursor",
        );

        let shape_capacity = anims.shape_indices.capacity();
        let entry_capacity = anims.entries.capacity();
        anims.clear();
        assert!(anims.shape_indices.is_empty());
        assert!(anims.entries.is_empty());
        assert_eq!(anims.shape_indices.capacity(), shape_capacity);
        assert_eq!(anims.entries.capacity(), entry_capacity);
    }

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
