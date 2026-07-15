//! Frame-entry types for the [`Ui`](crate::ui::Ui) lifecycle: the
//! retained lifecycle state ([`FrameRuntime`]), per-frame timestamp
//! ([`FrameStamp`]), wake-cause bitset + queue entry ([`WakeReasons`] /
//! [`Wake`]), and the per-frame plan ([`FramePlan`]) the entry classifier
//! picks. The frame *output* (what a frame produced) lives in
//! [`super::frame_report`].

use std::time::Duration;

use crate::display::Display;

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

/// WindowRenderer-supplied per-frame inputs — monotonic time + active
/// [`Display`]. Single struct so callers pass one argument and
/// `Ui` carries one `Option<FrameStamp>` for prior-frame state
/// instead of two parallel fields. `time` is the host's monotonic
/// clock (driven by the same source between frames); `display`
/// carries the surface size + scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStamp {
    pub display: Display,
    pub time: Duration,
}

impl FrameStamp {
    pub fn new(display: Display, time: Duration) -> Self {
        Self { display, time }
    }
}

/// One entry on the `Ui` repaint-wake queue.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wake {
    pub(crate) deadline: Duration,
    pub(crate) reasons: WakeReasons,
}

/// Retained clock, scheduling, and validity state owned by [`Ui`](crate::Ui).
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
    /// WindowRenderer-supplied monotonic timestamp for this frame.
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
    /// Whether the last painted frame reached its target successfully.
    /// The renderer acknowledges successful submits and `Ui::frame`
    /// acknowledges skips; a missing acknowledgement forces a full repaint
    /// so damage is never diffed against pixels that did not reach screen.
    pub(crate) frame_submitted: bool,
    /// Whether the current frame requires one settling record pass. The
    /// lifecycle consumes at most one such request per frame.
    pub(crate) relayout_requested: bool,
}

/// What `Ui::frame` should do this frame, decided at entry
/// from fired wake reasons + input state + prior-frame validity.
/// `PaintOnly` and `FullRecord` are mutually exclusive by construction
/// — `paint_only ⇒ !force_full` is encoded in the variant shape
/// instead of relying on two independent bools.
#[derive(Clone, Copy, Debug)]
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
