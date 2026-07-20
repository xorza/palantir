//! Frame-entry types for the [`Ui`](crate::ui::Ui) lifecycle: the
//! retained lifecycle state ([`FrameRuntime`]), per-frame timestamp
//! ([`FrameStamp`]), wake-cause bitset + queue entry ([`WakeReasons`] /
//! [`Wake`]), and the per-frame plan ([`FramePlan`]) the entry classifier
//! picks. The frame *output* (what a frame produced) lives in
//! [`super::frame_report`].

use std::time::Duration;

use crate::common::time::{ANIM_SUBSTEP_DT, MAX_ANIM_DT, coalesce_dt_for_refresh};
use crate::display::Display;
use crate::input::policy::InputPolicy;
use crate::primitives::approx::EPS;

/// Bitset over wake causes. OR-merged when two requests coalesce
/// onto the same deadline slot, so the frame-entry classifier can see
/// every reason behind a fired wake — used to pick `Full` vs
/// `AnimOnly` processing in `Ui::frame`. Bit set, not enum, because
/// a single deadline can legitimately have both bits at once
/// (paint-anim quantum aligning with a widget-scheduled wake).
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct WakeReasons(u8);

impl WakeReasons {
    /// Caller asked for a wake via `Ui::request_repaint_after` —
    /// state-spring tick, host-driven schedule, widget that owes a
    /// future paint. Requires a full record + measure + arrange +
    /// cascade pass.
    pub(crate) const REAL: Self = Self(1 << 0);
    /// Paint-anim quantum boundary, filed in `Ui::post_record` from
    /// `Forest::post_record`'s `min_wake`. On its own, only needs a
    /// damage compute + paint — record/post-record output from the
    /// prior frame is reused as-is.
    pub(crate) const ANIM: Self = Self(1 << 1);

    #[inline]
    pub(crate) fn merge(self, r: Self) -> Self {
        Self(self.0 | r.0)
    }

    /// `true` when the only reason set is `ANIM` — the predicate that
    /// gates `FrameProcessing::PaintOnly`.
    #[inline]
    pub(crate) fn is_anim_only(self) -> bool {
        self == Self::ANIM
    }
}

/// WindowDriver-supplied per-frame inputs — monotonic time + active
/// [`Display`]. Single struct so callers pass one argument and
/// `Ui` carries one `Option<FrameStamp>` for prior-frame state
/// instead of two parallel fields. `time` is the host's monotonic
/// clock (driven by the same source between frames); `display`
/// carries the surface size + scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrameStamp {
    pub(crate) display: Display,
    pub(crate) time: Duration,
}

impl FrameStamp {
    pub(crate) fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameInput {
    pub(crate) stamp: FrameStamp,
    pub(crate) damage_baseline_valid: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameClassifyInput {
    pub(crate) display: Display,
    pub(crate) damage_baseline_valid: bool,
    pub(crate) input_policy: InputPolicy,
    pub(crate) had_input: bool,
    pub(crate) input_requested_repaint: bool,
    pub(crate) close_requested: bool,
}

/// One entry on the `Ui` repaint-wake queue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}

/// Retained clock and scheduling state owned by [`Ui`](crate::Ui).
/// Grouping these fields keeps the frame lifecycle's reset and carry-over
/// invariants separate from the retained widget engines on `Ui`.
#[derive(Debug, Default)]
pub(crate) struct FrameRuntime {
    /// Effective per-frame dt fed into the animation integrators
    /// (`AnimMapTyped::tick` / `spring::step`). Real wall-clock dt is
    /// accumulated into [`Self::dt_accum`] and only spent here once it
    /// crosses [`crate::common::time::ANIM_SUBSTEP_DT`] — frames that
    /// do not spend see `dt = 0.0` and skip animation advancement.
    /// Without this, an unthrottled repaint loop can produce deltas
    /// below the f32 ULP at pixel-scale positions and stall a spring
    /// short of its settle threshold indefinitely.
    pub(crate) dt: f32,
    /// Unspent wall-clock dt waiting to cross the fixed-step threshold.
    /// See [`Self::dt`].
    pub(crate) dt_accum: f32,
    /// Bumped once per [`crate::Ui::frame`], before either record pass,
    /// so a settling pass cannot double-advance animation.
    pub(crate) frame_id: u64,
    /// WindowDriver-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// Time + display from the previous frame, or `None` before the
    /// first frame. Drives surface-change classification and the
    /// paint-animation damage gate.
    pub(crate) prev_stamp: Option<FrameStamp>,
    /// Fingerprint of the last frame's cascade inputs. A match permits
    /// reuse of the frozen cascade output; `None` before the first run.
    pub(crate) prev_cascade_fp: Option<u64>,
    /// Whether the most recent `post_record` ran the cascade, used to pin
    /// the unchanged-frame skip gate.
    #[cfg(test)]
    pub(crate) dbg_cascade_ran: bool,
    /// EMA of `1/raw_dt` across frames; zero before a second timestamp
    /// exists. Uses unclamped wall time so stalls remain visible.
    pub(crate) fps_ema: f32,
    /// Set when an unsettled animation or widget requests another frame.
    pub(crate) repaint_requested: bool,
    /// Pending absolute wake deadlines, sorted ascending and coalesced.
    /// Entries retain merged [`WakeReasons`] so coincident real and
    /// paint-animation wakes still force a full record pass.
    pub(crate) repaint_wakes: Vec<Wake>,
    /// Whether the current frame requires one settling record pass. The
    /// lifecycle consumes at most one such request per frame.
    pub(crate) relayout_requested: bool,
}

/// What `Ui::frame` should do this frame, decided at entry
/// from fired wake reasons + input state + prior-frame validity.
/// `PaintOnly` and `FullRecord` are mutually exclusive by construction
/// — `paint_only ⇒ !force_full` is encoded in the variant shape
/// instead of relying on two independent bools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FramePlan {
    /// Skip pre_record / record / finalize / layout / cascade and
    /// reuse the retained tree + cascades from the prior frame. Only
    /// fired by the anim-only fast path.
    PaintOnly,
    /// Run record + (optional) double-layout + finalize. `force_full`
    /// is true when the prior frame's damage snapshot must be
    /// discarded (surface change, missed submit, first frame).
    FullRecord { force_full: bool },
}

impl FrameRuntime {
    pub(crate) const MAX_DT: f32 = MAX_ANIM_DT;

    pub(crate) fn advance_clock(&mut self, now: Duration) {
        let true_dt = now.saturating_sub(self.time).as_secs_f32();
        let raw_dt = true_dt.min(Self::MAX_DT);
        if self.frame_id > 0 && true_dt > EPS {
            let instant_fps = 1.0 / true_dt;
            self.fps_ema = if self.fps_ema == 0.0 {
                instant_fps
            } else {
                self.fps_ema * 0.9 + instant_fps * 0.1
            };
        }
        self.dt_accum += raw_dt;
        self.dt = if self.dt_accum >= ANIM_SUBSTEP_DT {
            let spent = self.dt_accum;
            self.dt_accum = 0.0;
            spent
        } else {
            0.0
        };
        self.time = now;
        self.frame_id += 1;
    }

    pub(crate) fn classify_frame(&mut self, input: FrameClassifyInput) -> FramePlan {
        let fired_count = self
            .repaint_wakes
            .partition_point(|wake| wake.deadline <= self.time);
        let fired_reasons = self
            .repaint_wakes
            .drain(..fired_count)
            .fold(WakeReasons::default(), |acc, wake| acc.merge(wake.reasons));

        let first_frame = self.prev_stamp.is_none();
        let display_changed = self
            .prev_stamp
            .is_some_and(|previous| !previous.display.raster_eq(&input.display));
        let force_full = first_frame || display_changed || !input.damage_baseline_valid;
        if force_full {
            tracing::debug!(
                display_changed,
                damage_baseline_invalid = !input.damage_baseline_valid,
                first_frame,
                "damage.invalidate_prev"
            );
        }

        let input_forces_record = match input.input_policy {
            InputPolicy::Always => input.had_input,
            InputPolicy::OnDelta => input.input_requested_repaint,
        };
        let paint_only = !force_full
            && !self.repaint_requested
            && !input_forces_record
            && !input.close_requested
            && fired_reasons.is_anim_only();
        if paint_only {
            FramePlan::PaintOnly
        } else {
            FramePlan::FullRecord { force_full }
        }
    }

    pub(crate) fn schedule_wake(
        &mut self,
        deadline: Duration,
        reasons: WakeReasons,
        refresh_millihertz: Option<u32>,
    ) {
        let coalesce = coalesce_dt_for_refresh(refresh_millihertz);
        let near = |existing: Duration| existing.abs_diff(deadline) < coalesce;
        let position = self
            .repaint_wakes
            .partition_point(|wake| wake.deadline < deadline);
        if position < self.repaint_wakes.len() && near(self.repaint_wakes[position].deadline) {
            self.repaint_wakes[position].reasons =
                self.repaint_wakes[position].reasons.merge(reasons);
            return;
        }
        if position > 0 && near(self.repaint_wakes[position - 1].deadline) {
            self.repaint_wakes[position - 1].deadline = deadline;
            self.repaint_wakes[position - 1].reasons =
                self.repaint_wakes[position - 1].reasons.merge(reasons);
            return;
        }
        self.repaint_wakes
            .insert(position, Wake { deadline, reasons });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::UVec2;

    use crate::display::Display;
    use crate::input::policy::InputPolicy;
    use crate::ui::frame::{FrameClassifyInput, FramePlan, FrameRuntime, FrameStamp, WakeReasons};

    #[derive(Clone, Copy, Debug)]
    struct Case {
        label: &'static str,
        previous: bool,
        display_changed: bool,
        damage_baseline_valid: bool,
        wake: WakeReasons,
        repaint_requested: bool,
        input_policy: InputPolicy,
        had_input: bool,
        input_requested_repaint: bool,
        close_requested: bool,
        expected: FramePlan,
    }

    #[test]
    fn frame_classification_covers_external_entry_facts() {
        let cases = [
            Case {
                label: "first frame",
                previous: false,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::default(),
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: true },
            },
            Case {
                label: "display change",
                previous: true,
                display_changed: true,
                damage_baseline_valid: true,
                wake: WakeReasons::default(),
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: true },
            },
            Case {
                label: "invalid prior output",
                previous: true,
                display_changed: false,
                damage_baseline_valid: false,
                wake: WakeReasons::ANIM,
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: true },
            },
            Case {
                label: "animation wake",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::ANIM,
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::PaintOnly,
            },
            Case {
                label: "real wake",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::REAL,
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: false },
            },
            Case {
                label: "coalesced real and animation wake",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::REAL.merge(WakeReasons::ANIM),
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: false },
            },
            Case {
                label: "always input policy",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::ANIM,
                repaint_requested: false,
                input_policy: InputPolicy::Always,
                had_input: true,
                input_requested_repaint: false,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: false },
            },
            Case {
                label: "delta input policy",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::ANIM,
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: true,
                input_requested_repaint: true,
                close_requested: false,
                expected: FramePlan::FullRecord { force_full: false },
            },
            Case {
                label: "close request",
                previous: true,
                display_changed: false,
                damage_baseline_valid: true,
                wake: WakeReasons::ANIM,
                repaint_requested: false,
                input_policy: InputPolicy::OnDelta,
                had_input: false,
                input_requested_repaint: false,
                close_requested: true,
                expected: FramePlan::FullRecord { force_full: false },
            },
        ];

        let base_display = Display::from_physical(UVec2::new(100, 80), 1.0);
        for case in cases {
            let display = if case.display_changed {
                Display::from_physical(UVec2::new(101, 80), 1.0)
            } else {
                base_display
            };
            let mut runtime = FrameRuntime {
                time: Duration::from_millis(10),
                prev_stamp: case
                    .previous
                    .then_some(FrameStamp::new(base_display, Duration::ZERO)),
                repaint_requested: case.repaint_requested,
                ..FrameRuntime::default()
            };
            if case.wake != WakeReasons::default() {
                runtime.schedule_wake(Duration::from_millis(10), case.wake, None);
            }

            let actual = runtime.classify_frame(FrameClassifyInput {
                display,
                damage_baseline_valid: case.damage_baseline_valid,
                input_policy: case.input_policy,
                had_input: case.had_input,
                input_requested_repaint: case.input_requested_repaint,
                close_requested: case.close_requested,
            });

            assert_eq!(actual, case.expected, "{}", case.label);
        }
    }
}
