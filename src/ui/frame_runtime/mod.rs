//! Retained clock and scheduling state for the `Ui` frame lifecycle, and the
//! plan the frame's entry decision produces. A frame's output lives in
//! [`frame_report`](crate::ui::frame_report).

pub(crate) mod wake;

use crate::common::time::{self, ANIMATION_SUBSTEP_DT, MAX_ANIMATION_DT};
use crate::damage::DamageBase;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::input::policy::{InputPolicy, InputSignal};
use crate::primitives::math::domain::EPS;
use crate::text::shaper::TextShaper;
use crate::ui::frame_report::FrameProcessing;
use crate::ui::frame_runtime::wake::{Wake, WakeReasons};
use std::mem;
use std::time::Duration;

/// What `Ui::frame` does this frame, decided at entry from fired wake reasons,
/// input state and prior-frame validity. The variants make a paint-only frame
/// with a discarded damage base unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FramePlan {
    /// Skip pre_record, record, finalize, layout and cascade; reuse the prior
    /// frame's tree and cascade. Fired only by the anim-only fast path.
    PaintOnly,
    /// Run record, optional double-layout and finalize, diffing damage against
    /// `base`.
    FullRecord { base: DamageBase },
}

/// What a recorded tree was built under besides its authoring: the theme the widgets read and the
/// fonts its text shaped with. Neither moves a key a paint-only frame checks, so a move forces a
/// record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RecordBaseline {
    /// Bumped by every theme write (`Ui::set_theme`).
    pub(super) theme_epoch: u32,
    /// `TextShaper::font_epoch`, which a font load in any window sharing the shaper moves.
    pub(super) font_epoch: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameClassifyInput {
    pub(super) display: Display,
    pub(super) record_baseline: RecordBaseline,
    pub(super) damage_baseline_valid: bool,
    pub(super) input_policy: InputPolicy,
    pub(super) input_signal: InputSignal,
    pub(super) close_requested: bool,
}

/// Retained clock and scheduling state owned by [`Ui`](crate::Ui).
#[derive(Debug, Default)]
pub(crate) struct FrameRuntime {
    /// Effective per-frame dt fed to the animation integrators. Wall-clock dt
    /// accumulates in [`Self::dt_accum`] and is spent here only once it crosses
    /// [`crate::common::time::ANIMATION_SUBSTEP_DT`]; other frames see `0.0`.
    /// Otherwise an unthrottled loop produces deltas below the f32 ULP at
    /// pixel-scale positions and stalls a spring short of settling.
    pub(super) dt: f32,
    /// Unspent wall-clock dt waiting to cross the threshold; see [`Self::dt`].
    pub(super) dt_accum: f32,
    /// Bumped once per [`crate::Ui::frame`], before either record pass, so a
    /// settling pass cannot double-advance animation. Counts `PaintOnly`
    /// frames too, unlike [`Self::frame_id`].
    pub(super) render_frame_id: u64,
    /// This frame's display and host-supplied monotonic time, set by [`Self::begin_frame`].
    pub(super) stamp: FrameStamp,
    /// Time and display from the previous frame, `None` before the first;
    /// drives surface-change classification and the paint-animation damage gate.
    pub(super) prev_stamp: Option<FrameStamp>,
    /// EMA of `1/raw_dt`; zero before a second timestamp. Uses unclamped wall
    /// time so stalls stay visible.
    pub(super) fps_ema: f32,
    /// Full-record frames so far, published as [`crate::Ui::frame_id`].
    /// Bumped in [`Self::note_processing`], so inside a record pass it counts
    /// the record frames before this one: consecutive frames see consecutive
    /// values and both passes of one frame see the same one.
    pub(super) frame_id: u64,
    /// How many of [`Self::frame_id`]'s frames needed a settling second pass.
    /// Cumulative so a caller reads the delta across a gesture. `PaintOnly`
    /// frames are excluded from both halves of the ratio. Shown by the
    /// frame-stats overlay.
    settle_frames: u32,
    /// Set when an unsettled animation or widget requests another frame.
    repaint_requested: bool,
    /// The shared text clock's reading when this window last framed; only
    /// touched by [`Self::tick_text_clock`].
    text_frame: Option<u64>,
    /// Pending absolute wake deadlines, sorted ascending and coalesced, with
    /// merged [`WakeReasons`] so coincident wakes still force a full record.
    repaint_wakes: Vec<Wake>,
    /// Whether the current frame needs one settling record pass; at most one
    /// per frame.
    relayout_requested: bool,
    /// The baseline the retained tree was last recorded under, `None` before the first record.
    recorded: Option<RecordBaseline>,
}

impl FrameRuntime {
    /// Advance the shared text clock when this window frames again on the
    /// reading it last framed at.
    ///
    /// A window frames at most once per host frame, so framing again on the
    /// same reading proves a host frame passed. N windows painting together
    /// tick once per round, and no host can forget the tick (see
    /// [`TextShaper::tick_frame`] for the stall that causes). Every frame
    /// reaches here, `PaintOnly` included, so none skips it or pays it twice.
    pub(super) fn tick_text_clock(&mut self, text: &TextShaper) {
        if self.text_frame == Some(text.frame()) {
            text.tick_frame();
        }
        self.text_frame = Some(text.frame());
    }

    /// Fold this frame's outcome into [`Self::frame_id`] and the settle tally.
    /// Called once per [`crate::Ui::frame`] after the pass count is known, so
    /// the overlay always reads both through the previous frame.
    pub(super) const fn note_processing(&mut self, processing: FrameProcessing) {
        match processing {
            FrameProcessing::PaintOnly => {}
            FrameProcessing::SingleLayout => self.frame_id += 1,
            FrameProcessing::DoubleLayout => {
                self.frame_id += 1;
                self.settle_frames += 1;
            }
        }
    }

    /// Open the frame `stamp` describes: its display stands for the frame, and the clock advances
    /// to its time.
    pub(super) fn begin_frame(&mut self, stamp: FrameStamp) {
        self.advance_clock(stamp.time);
        self.stamp.display = stamp.display;
    }

    /// Close the frame: its stamp becomes the previous one the next frame compares with.
    pub(super) const fn end_frame(&mut self) {
        self.prev_stamp = Some(self.stamp);
    }

    /// This frame's display and time.
    pub(crate) const fn stamp(&self) -> FrameStamp {
        self.stamp
    }

    /// How many record frames needed a settling second pass so far.
    pub(crate) const fn settle_frames(&self) -> u32 {
        self.settle_frames
    }

    /// The earliest pending wake's deadline, when one is pending.
    pub(super) fn next_wake(&self) -> Option<Duration> {
        self.repaint_wakes.first().map(|wake| wake.deadline)
    }

    /// Ask for another frame after this one.
    pub(super) const fn request_repaint(&mut self) {
        self.repaint_requested = true;
    }

    /// Whether another frame is owed after this one.
    pub(super) const fn repaint_requested(&self) -> bool {
        self.repaint_requested
    }

    /// Ask for a settling record pass in this frame.
    pub(super) const fn request_relayout(&mut self) {
        self.relayout_requested = true;
    }

    /// Whether this frame asked for a settling record pass.
    pub(super) const fn relayout_requested(&self) -> bool {
        self.relayout_requested
    }

    /// Open a record pass with no settling pass asked.
    pub(super) const fn begin_pass(&mut self) {
        self.relayout_requested = false;
    }

    /// Drop what a discarded pass asked for: a settling pass and another frame.
    pub(super) const fn withdraw_requests(&mut self) {
        self.relayout_requested = false;
        self.repaint_requested = false;
    }

    pub(super) fn advance_clock(&mut self, now: Duration) {
        let true_dt = now.saturating_sub(self.stamp.time).as_secs_f32();
        let raw_dt = true_dt.min(MAX_ANIMATION_DT);
        if self.render_frame_id > 0 && true_dt > EPS {
            let instant_fps = 1.0 / true_dt;
            self.fps_ema = if self.fps_ema == 0.0 {
                instant_fps
            } else {
                self.fps_ema * 0.9 + instant_fps * 0.1
            };
        }
        self.dt_accum += raw_dt;
        self.dt = if self.dt_accum >= ANIMATION_SUBSTEP_DT {
            let spent = self.dt_accum;
            self.dt_accum = 0.0;
            // Bounds what one frame spends (this plus a carry of up to one
            // accumulator step); `Spring::step` takes `MAX_ANIMATION_DT` as its
            // contract. The excess is dropped, not carried, so a stall cannot
            // bleed into later frames as catch-up motion.
            spent.min(MAX_ANIMATION_DT)
        } else {
            0.0
        };
        self.stamp.time = now;
        self.render_frame_id += 1;
    }

    /// No frame has been stamped yet: no previous display to compare and no
    /// retained pixels. Shared by the plan classifier and `FrameCycle::run`.
    pub(crate) const fn is_first_frame(&self) -> bool {
        self.prev_stamp.is_none()
    }

    /// Decide what this frame does, consuming the wakes that fired by now (a
    /// wake drives exactly one frame). Named `take_` because a reader may
    /// assume `classify_*` is pure.
    pub(super) fn take_frame_plan(&mut self, input: FrameClassifyInput) -> FramePlan {
        let fired_count = self
            .repaint_wakes
            .partition_point(|wake| wake.deadline <= self.stamp.time);
        let fired_reasons = self
            .repaint_wakes
            .drain(..fired_count)
            .fold(WakeReasons::default(), |acc, wake| acc.merge(wake.reasons));

        let first_frame = self.is_first_frame();
        let display_changed = self
            .prev_stamp
            .is_some_and(|prev| !prev.display.raster_eq(&input.display));
        let force_full = first_frame || display_changed || !input.damage_baseline_valid;
        if force_full {
            tracing::debug!(
                display_changed,
                damage_baseline_invalid = !input.damage_baseline_valid,
                first_frame,
                "damage.invalidate_prev"
            );
        }

        // The policy names a cut on `InputSignal`'s ordered scale; the gate is
        // the comparison.
        let input_forces_record = input.input_signal >= input.input_policy.record_threshold();
        // Consumed like the wakes: a request drives exactly one frame. Taking it
        // here keeps the field to one meaning (before: someone asked; after:
        // this frame asked for another).
        let repaint_requested = mem::take(&mut self.repaint_requested);
        let baseline_moved = self.recorded != Some(input.record_baseline);
        let paint_only = !force_full
            && !baseline_moved
            && !repaint_requested
            && !input_forces_record
            && !input.close_requested
            && fired_reasons.is_animation_only();
        if paint_only {
            FramePlan::PaintOnly
        } else {
            self.recorded = Some(input.record_baseline);
            FramePlan::FullRecord {
                base: if force_full {
                    DamageBase::Discarded
                } else {
                    DamageBase::Prev
                },
            }
        }
    }

    pub(super) fn schedule_wake(
        &mut self,
        deadline: Duration,
        reasons: WakeReasons,
        refresh_millihertz: Option<u32>,
    ) {
        let coalesce = time::coalesce_dt_for_refresh(refresh_millihertz);
        let near = |existing: Duration| existing.abs_diff(deadline) < coalesce;
        let pos = self
            .repaint_wakes
            .partition_point(|wake| wake.deadline < deadline);
        if pos < self.repaint_wakes.len() && near(self.repaint_wakes[pos].deadline) {
            self.repaint_wakes[pos].reasons = self.repaint_wakes[pos].reasons.merge(reasons);
            return;
        }
        if pos > 0 && near(self.repaint_wakes[pos - 1].deadline) {
            self.repaint_wakes[pos - 1].deadline = deadline;
            self.repaint_wakes[pos - 1].reasons =
                self.repaint_wakes[pos - 1].reasons.merge(reasons);
            return;
        }
        self.repaint_wakes.insert(pos, Wake { deadline, reasons });
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::display::Display;
    use crate::display::frame_stamp::FrameStamp;
    use crate::ui::frame_runtime::FrameRuntime;
    #[cfg(test)]
    use crate::ui::frame_runtime::wake::Wake;

    impl FrameRuntime {
        /// The pending wakes, earliest first.
        #[cfg(test)]
        pub(crate) fn repaint_wakes(&self) -> &[Wake] {
            &self.repaint_wakes
        }

        /// Set the last frame's stamp. `Some` makes the next frame warm (skips warmup); `None`
        /// is a cold start.
        pub(crate) const fn set_prev_stamp(&mut self, stamp: Option<FrameStamp>) {
            self.prev_stamp = stamp;
        }

        /// Replace the display the next frame lays out at, as the window driver does.
        pub(crate) const fn set_display(&mut self, display: Display) {
            self.stamp.display = display;
        }
    }
}

#[cfg(test)]
mod tests;
