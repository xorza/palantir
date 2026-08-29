//! Retained clock and scheduling state for the `Ui` frame lifecycle, and the
//! plan the frame's entry decision produces.
//!
//! The frame *output* (what a frame produced) lives in
//! [`frame_report`](crate::ui::frame_report).

pub(crate) mod wake;

use crate::common::counters::TestOnly;
use crate::common::time::{ANIM_SUBSTEP_DT, MAX_ANIM_DT, coalesce_dt_for_refresh};
use crate::display::Display;
use crate::input::policy::{InputPolicy, InputSignal};
use crate::primitives::approx::EPS;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::wake::{Wake, WakeReasons};
use crate::ui::frame_stamp::FrameStamp;
use std::time::Duration;

/// What `Ui::frame` should do this frame, decided at entry
/// from fired wake reasons + input state + prior-frame validity.
/// `PaintOnly` and `FullRecord` are mutually exclusive by construction
/// — `paint_only ⇒ !force_full` is encoded in the variant shape
/// instead of relying on two independent bools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FramePlan {
    /// Skip pre_record / record / finalize / layout / cascade and
    /// reuse the retained tree + cascade from the prior frame. Only
    /// fired by the anim-only fast path.
    PaintOnly,
    /// Run record + (optional) double-layout + finalize. `force_full`
    /// is true when the prior frame's damage snapshot must be
    /// discarded (surface change, missed submit, first frame).
    FullRecord { force_full: bool },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameClassifyInput {
    pub(super) display: Display,
    pub(super) damage_baseline_valid: bool,
    pub(super) input_policy: InputPolicy,
    pub(super) input_signal: InputSignal,
    pub(super) close_requested: bool,
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
    /// so a settling pass cannot double-advance animation. Counts every
    /// frame that reaches the screen, `PaintOnly` ones included —
    /// [`Self::frame_id`] is the peer that counts only the frames
    /// authoring code ran on.
    pub(crate) render_frame_id: u64,
    /// WindowDriver-supplied monotonic timestamp for this frame.
    pub(crate) time: Duration,
    /// Time + display from the previous frame, or `None` before the
    /// first frame. Drives surface-change classification and the
    /// paint-animation damage gate.
    pub(crate) prev_stamp: Option<FrameStamp>,
    /// Fingerprint of the last frame's cascade inputs. A match permits
    /// reuse of the frozen cascade output; `None` before the first run.
    pub(crate) prev_cascade_fp: Option<u64>,
    /// Whether the most recent `post_record` ran the cascade — pins the
    /// unchanged-frame skip gate.
    ///
    /// A [`TestOnly`] cell, like every other probe in the crate: the gate
    /// lives in the cell, so [`Self::note_cascade_ran`] is an ordinary
    /// method and its call site carries nothing.
    cascade_ran: TestOnly<bool>,
    /// EMA of `1/raw_dt` across frames; zero before a second timestamp
    /// exists. Uses unclamped wall time so stalls remain visible.
    pub(crate) fps_ema: f32,
    /// Full-record frames so far — the frame identity authoring code
    /// sees, published as [`crate::Ui::frame_id`], since a `PaintOnly`
    /// frame runs none of it.
    ///
    /// Bumped in [`Self::note_processing`] rather than beside
    /// [`Self::render_frame_id`], so read from inside a record pass it
    /// counts the record frames *before* this one. Two consecutive
    /// record frames therefore observe consecutive values, and both
    /// passes of one frame observe the same one — which is what makes it
    /// usable as an identity and not merely a tally.
    pub(crate) frame_id: u64,
    /// How many of [`Self::frame_id`]'s frames needed a settling second
    /// record pass. Cumulative rather than an EMA because the question it
    /// answers is "did this gesture stop double-recording" — you read the
    /// *delta* across an interaction, which a decaying average smears.
    /// `PaintOnly` frames can't settle, so they are excluded from both
    /// halves of that ratio rather than drifting it toward zero while the
    /// UI merely idles. Displayed by the opt-in frame-stats overlay.
    pub(crate) settle_frames: u32,
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

impl FrameRuntime {
    /// Record whether `post_record` ran the cascade this frame. Same
    /// principle as the probe structs: the gate lives here, so the call
    /// site in `FrameCycle::post_record` carries none.
    #[inline]
    pub(crate) fn note_cascade_ran(&mut self, ran: bool) {
        self.cascade_ran.edit(|noted| *noted = ran);
    }

    /// Whether the last `post_record` ran the cascade — pins the
    /// unchanged-frame skip gate.
    #[cfg(test)]
    pub(crate) fn cascade_ran(&self) -> bool {
        *self.cascade_ran.get()
    }

    /// Fold this frame's outcome into [`Self::frame_id`] and the settle
    /// tally. Called once per [`crate::Ui::frame`], after the pass count is
    /// known — so the overlay, which records *during* a pass, always reads
    /// both through the previous frame.
    pub(super) fn note_processing(&mut self, processing: FrameProcessing) {
        match processing {
            FrameProcessing::PaintOnly => {}
            FrameProcessing::SingleLayout => self.frame_id += 1,
            FrameProcessing::DoubleLayout => {
                self.frame_id += 1;
                self.settle_frames += 1;
            }
        }
    }

    pub(super) fn advance_clock(&mut self, now: Duration) {
        let true_dt = now.saturating_sub(self.time).as_secs_f32();
        let raw_dt = true_dt.min(MAX_ANIM_DT);
        if self.render_frame_id > 0 && true_dt > EPS {
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
        self.render_frame_id += 1;
    }

    /// Decide what this frame does, **consuming** the wakes that fired by
    /// now — the drain is the point, not a side effect, since a wake must
    /// drive exactly one frame. Named `take_` for that reason: a reader is
    /// entitled to assume a `classify_*` is pure, and this is the frame's
    /// single entry decision.
    /// No frame has been stamped yet, so there is no previous display to
    /// compare against and no retained pixels to keep.
    ///
    /// Named rather than spelled `prev_stamp.is_none()` at each of the two
    /// sites that ask — the plan classifier below, and `FrameCycle::run`,
    /// which gates its warmup pass and its damage assertion on the same
    /// fact.
    pub(crate) fn is_first_frame(&self) -> bool {
        self.prev_stamp.is_none()
    }

    pub(super) fn take_frame_plan(&mut self, input: FrameClassifyInput) -> FramePlan {
        let fired_count = self
            .repaint_wakes
            .partition_point(|wake| wake.deadline <= self.time);
        let fired_reasons = self
            .repaint_wakes
            .drain(..fired_count)
            .fold(WakeReasons::default(), |acc, wake| acc.merge(wake.reasons));

        let first_frame = self.is_first_frame();
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

        // The policy names a cut on `InputSignal`'s ordered scale; the
        // gate is the comparison.
        let input_forces_record = input.input_signal >= input.input_policy.record_threshold();
        // Consumed, like the wakes above and for the same reason: a
        // request drives exactly one frame. Taking it here rather than
        // clearing it in `FrameCycle::run` a few lines later is what
        // keeps the field to one meaning — before this call it is
        // "someone has asked for a frame", after it is "this frame asked
        // for another". Clearing it separately left both meanings live
        // on one field, told apart only by statement order.
        let repaint_requested = std::mem::take(&mut self.repaint_requested);
        let paint_only = !force_full
            && !repaint_requested
            && !input_forces_record
            && !input.close_requested
            && fired_reasons.is_anim_only();
        if paint_only {
            FramePlan::PaintOnly
        } else {
            FramePlan::FullRecord { force_full }
        }
    }

    pub(super) fn schedule_wake(
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
mod tests;
