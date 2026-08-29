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

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::scene::tree::node_id::NodeId;
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
    /// repeating from `started_at` until `stop_after` has elapsed, then
    /// solid forever. The caret-blink shape.
    ///
    /// The stop lives here rather than in the widget that registers the
    /// anim because it has to hold on frames the widget never sees. A
    /// blinking caret is enough on its own to wake the host, and such a
    /// wake produces a paint-only frame — no record pass, so no widget
    /// code runs to re-decide anything. An idle cutoff evaluated at
    /// record time is therefore evaluated exactly once and never again,
    /// and the blink runs forever. Sampled here, it settles on the
    /// paint pass that crosses it and [`Self::next_wake`] stops asking
    /// for frames.
    BlinkOpacity {
        half_period: Duration,
        started_at: Duration,
        /// Idle span from `started_at` after which the blink settles
        /// solid. [`Duration::MAX`] blinks indefinitely.
        stop_after: Duration,
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
    /// transform). The encoder folds it and the pivot into the payload's
    /// [`StrokeBounds`](crate::renderer::frontend::payload::stroke_bounds::StrokeBounds).
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
                stop_after,
            } => {
                let alpha = if blink_visible_at(half_period, started_at, stop_after, now) {
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

    /// Whether this anim turns the shape's geometry, which decides
    /// whether a bound has to cover the swept disc rather than the
    /// recorded bbox.
    #[inline]
    pub(crate) fn rotates(self) -> bool {
        matches!(self, PaintAnim::Spin { .. })
    }

    /// Earliest `Duration` (absolute time, same epoch as
    /// frame-runtime time / `started_at`) at which `quantum` will next
    /// change, or `None` when the anim has settled and will never
    /// change again. `post_record` folds the min of every live entry's
    /// `next_wake` into the frame runtime's wake queue so widgets don't have to.
    ///
    /// For `BlinkOpacity` this is the next half-period boundary
    /// strictly after `now`, until `stop_after` elapses.
    #[inline]
    pub(crate) fn next_wake(self, now: Duration) -> Option<Duration> {
        match self {
            PaintAnim::BlinkOpacity {
                half_period,
                started_at,
                stop_after,
            } => next_blink_boundary(half_period, started_at, stop_after, now),
            // Continuous: the angle changes every frame, so the soonest
            // it "next changes" is now. `extend_predamaged` compares
            // `next_wake(prev) <= now` (always true, since `prev <= now`)
            // and so re-damages the spun shape's rect every frame.
            PaintAnim::Spin { .. } => Some(now),
        }
    }
}

/// True when a blink with `half_period` starting at `started_at` is
/// in its solid phase at `now`. Pre-start (now < started_at) returns
/// `true` so a freshly-focused caret is immediately visible, and so
/// does everything from `started_at + stop_after` onwards.
#[inline]
fn blink_visible_at(
    half_period: Duration,
    started_at: Duration,
    stop_after: Duration,
    now: Duration,
) -> bool {
    if now <= started_at {
        return true;
    }
    let dt = now - started_at;
    if dt >= stop_after {
        return true;
    }
    // (dt / half_period) parity: even = solid, odd = hidden.
    let n = duration_div_floor(dt, half_period);
    n & 1 == 0
}

/// Absolute time of the next strictly-future boundary at which the
/// blink flips. Aligns to `started_at + k * half_period` for the
/// smallest `k` with that time `> now`. `None` once the blink has
/// settled solid — a degenerate zero period, or a boundary that would
/// land at or past `started_at + stop_after`.
#[inline]
fn next_blink_boundary(
    half_period: Duration,
    started_at: Duration,
    stop_after: Duration,
    now: Duration,
) -> Option<Duration> {
    if half_period.is_zero() {
        return None;
    }
    if now < started_at {
        return Some(started_at);
    }
    let settles_at = started_at.saturating_add(stop_after);
    if now >= settles_at {
        return None;
    }
    let dt = now - started_at;
    let n = duration_div_floor(dt, half_period);
    let boundary = started_at + half_period.saturating_mul((n + 1) as u32);
    // The settle is itself a flip the encoder has to paint, so it caps
    // the wake rather than merely bounding it: a `stop_after` that isn't
    // a whole number of half-periods lands between two boundaries, and
    // waking only on boundaries would leave the caret stuck on whatever
    // phase it was in until some unrelated wake arrived.
    Some(boundary.min(settles_at))
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
    /// Index into `Tree::shapes.records` of the animated shape. Strictly
    /// increasing across [`PaintAnims::entries`], because registration
    /// follows append-only shape recording — which is what lets both
    /// readers below find a shape's row by search rather than by scan.
    pub(crate) shape_idx: u32,
    /// Paint-arena row of the animated shape inside its owner's
    /// `node_spans` span — the chrome offset plus the shape's position
    /// in the owner's `TreeItems` stream, captured from
    /// `OpenFrame::paint_rows` at `add_shape_animated` time. Lets
    /// damage's `extend_predamaged` index the shape's screen rect as
    /// `paint_arena.rows[node_span.start + row]` with no per-frame
    /// `TreeItems` walk.
    pub(crate) row: u32,
    /// The node that owns this shape — the open node at
    /// `add_shape_animated` time. Lets the damage lookup index
    /// `node_spans[node]` directly without needing a per-frame
    /// `shape_idx → paint_idx` reverse map.
    pub(crate) node: NodeId,
}

/// Per-tree sparse paint-animation registry, cleared per frame.
#[derive(Debug, Default)]
pub(crate) struct PaintAnims {
    /// Live anim entries, in registration order — which is shape order,
    /// so `shape_idx` increases down the column. Iterated by
    /// `Forest::min_paint_anim_wake` (next-wake fold) and
    /// `DamageEngine::compute` (anim-damage union), searched by
    /// [`Self::rotates`], and walked in step by [`PaintAnimCursor`].
    pub(crate) entries: Vec<PaintAnimEntry>,
}

impl PaintAnims {
    /// Reset for a fresh recording frame. Capacity retained — same
    /// lifecycle as every other per-frame tree column.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Register one animation against the shape it was recorded on.
    pub(crate) fn push_entry(&mut self, entry: PaintAnimEntry) {
        debug_assert!(
            self.entries
                .last()
                .is_none_or(|last| last.shape_idx < entry.shape_idx),
            "paint animation shape indices must be strictly increasing",
        );
        self.entries.push(entry);
    }

    /// Whether the shape at `shape_idx` paints under a rotation.
    ///
    /// The cascade's question, and it asks it without a `now`: what a
    /// rotating shape is culled and damaged against is the square it
    /// sweeps, which is the same at every angle. `entries` is ordered by
    /// `shape_idx` and holds only the handful of shapes a frame
    /// animates, so the search is over a list that is usually empty.
    pub(crate) fn rotates(&self, shape_idx: u32) -> bool {
        self.entries
            .binary_search_by_key(&shape_idx, |entry| entry.shape_idx)
            .is_ok_and(|i| self.entries[i].anim.rotates())
    }

    pub(crate) fn cursor(&self) -> PaintAnimCursor<'_> {
        PaintAnimCursor {
            entries: &self.entries,
            next: 0,
            next_shape: next_shape(&self.entries, 0),
            #[cfg(debug_assertions)]
            last_sampled: None,
        }
    }
}

/// `entries[next]`'s shape index, or [`CURSOR_END`] past the last row.
#[inline]
fn next_shape(entries: &[PaintAnimEntry], next: usize) -> u64 {
    entries
        .get(next)
        .map_or(CURSOR_END, |entry| entry.shape_idx as u64)
}

/// Monotonic encoder lookup over the sparse animation rows.
#[derive(Debug)]
pub(crate) struct PaintAnimCursor<'a> {
    entries: &'a [PaintAnimEntry],
    next: usize,
    next_shape: u64,
    /// Previous [`Self::sample`] argument, so the monotonicity
    /// precondition is checked in debug rather than trusted.
    ///
    /// A backwards walk has no failure signal of its own: the cursor
    /// answers `IDENTITY`, which is also what an unanimated shape gets,
    /// and the registration it already stepped past stays consumed. The
    /// recording half asserts the same ordering in
    /// [`PaintAnims::push_entry`]; this is the reading half of it.
    #[cfg(debug_assertions)]
    last_sampled: Option<u32>,
}

impl PaintAnimCursor<'_> {
    /// `shape_idx` must increase between calls. Jumps are allowed because
    /// viewport and damage culling can skip whole shape ranges.
    #[inline]
    pub(crate) fn sample(&mut self, shape_idx: u32, now: Duration) -> PaintMod {
        #[cfg(debug_assertions)]
        {
            debug_assert!(
                self.last_sampled.is_none_or(|last| shape_idx > last),
                "paint-anim sampling must be monotonic — sampled {shape_idx} after \
                 {last:?}. Going backwards reads as an unanimated shape and leaves \
                 the registrations already stepped past consumed.",
                last = self.last_sampled,
            );
            self.last_sampled = Some(shape_idx);
        }
        let shape_idx = shape_idx as u64;
        while shape_idx > self.next_shape {
            self.advance();
        }
        // The loop lands on the first registration at or past `shape_idx`;
        // only an exact hit is this shape's. A jump that skips over one
        // registration and stops short of the next must not hand out the
        // next one's sample — and must not consume it either, or the
        // shape that owns it would then paint unanimated. `CURSOR_END`
        // is `u64::MAX`, which no `u32` index can equal, so an exhausted
        // cursor falls out here too.
        if shape_idx != self.next_shape {
            return PaintMod::IDENTITY;
        }
        let entry = self.entries[self.next];
        self.advance();
        entry.anim.sample(now)
    }

    #[inline]
    fn advance(&mut self) {
        self.next += 1;
        self.next_shape = next_shape(self.entries, self.next);
    }
}

#[cfg(test)]
mod tests;
