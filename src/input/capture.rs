//! The per-button press-capture state machine: what a press latched
//! onto, whether it has become a drag, how it ended, and how presses
//! chain into double- and triple-click runs.
//!
//! [`Capture`] is the whole of it — [`Press`], [`PressDrag`],
//! [`Release`], [`ReleaseKind`] and [`PressRun`] are its parts, and the
//! three tunables below are the thresholds it latches on. Kept together
//! because the invariants only hold across the set: a capture always has
//! a press origin, a drag latch always has a capture, click and
//! drag-stop never coexist, and the run tracker never half-exists.

use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use std::time::Duration;

/// Pointer travel from press origin (logical px) before a gesture
/// latches as a drag. Under this, the gesture is still a click. Once
/// crossed, the latch holds for the press lifetime and the release
/// no longer emits a click. Mouse-sized — touch will want larger.
pub(crate) const DRAG_THRESHOLD: f32 = 4.0;

/// Maximum interval between two clicks on the same widget for the
/// second one to be reported as a double-click. 500 ms matches the
/// Windows / Chromium default; macOS's `NSEvent.doubleClickInterval`
/// is user-configurable but defaults to the same neighborhood, and
/// Linux has no system-wide value to read. Tracked per-button on
/// [`Capture`].
pub(crate) const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// Maximum pointer travel (logical px) between two clicks for the second
/// to still count as a double-click. A slow drift past this reads as two
/// separate clicks, which matches native behaviour. Tracked per-button on
/// [`Capture`], and [`TextEdit`](crate::TextEdit)'s word and all-selection
/// read the run it bounds rather than keeping a radius of their own.
pub(super) const DOUBLE_CLICK_RADIUS: f32 = 5.0;

/// Per-button capture. One slot per
/// [`PointerButton`](crate::input::pointer::PointerButton); three
/// all-or-nothing pieces rather than twelve loose fields, so the
/// invariants (a capture always has a press origin, a drag latch always
/// has a capture, click and drag-stop never coexist, the run tracker
/// never half-exists) are unrepresentable rather than maintained by
/// convention.
#[derive(Default, Clone, Copy, Debug)]
pub(super) struct Capture {
    /// The in-flight press, created on the press event and destroyed
    /// by release / cascade-eviction. `Some` == "this button's
    /// capture is latched".
    pub(super) press: Option<Press>,
    /// One-frame edge: how a capture ended this frame. Cleared by
    /// `end_frame`.
    pub(super) release: Option<Release>,
    /// Multi-press run tracker. Persists *across* presses (that's the
    /// chaining) — never cleared, only replaced by the next press.
    pub(super) run: Option<PressRun>,
}

impl Capture {
    /// Latch a press on `target` at `pos`, chaining the multi-press
    /// run when it lands on the same target within
    /// [`DOUBLE_CLICK_WINDOW`] of the previous press and
    /// [`DOUBLE_CLICK_RADIUS`] of its position; any break restarts the
    /// run at 1. `seq` saturates so a caffeinated 255-click run can't
    /// wrap back to "single".
    pub(super) fn begin_press(&mut self, target: WidgetId, pos: Vec2, now: Duration) {
        let seq = match &self.run {
            Some(run)
                if run.target == target
                    && now.saturating_sub(run.at) <= DOUBLE_CLICK_WINDOW
                    && pos.distance(run.pos) <= DOUBLE_CLICK_RADIUS =>
            {
                run.seq.saturating_add(1)
            }
            _ => 1,
        };
        self.run = Some(PressRun {
            at: now,
            target,
            pos,
            seq,
        });
        self.press = Some(Press {
            target,
            origin: pos,
            seq,
            fresh: true,
            drag: PressDrag::None,
        });
    }
}

/// One in-flight press: the capture target, the drag anchor, and this
/// press's run position, bundled so none can exist without the others.
#[derive(Clone, Copy, Debug)]
pub(super) struct Press {
    /// Widget the press latched onto.
    pub(super) target: WidgetId,
    /// Pointer position at the press. Subtracted from the current
    /// pointer position for rect-independent drag deltas.
    pub(super) origin: Vec2,
    /// This press's position in its multi-press run (1 = single,
    /// 2 = double-press, 3+ = triple…), stamped from [`PressRun::seq`]
    /// at press time so the release can carry the click count without
    /// depending on the run tracker's later state.
    pub(super) seq: u8,
    /// One-frame edge: the press landed this frame (drives
    /// `ButtonPhase::Down`). Lowered by `drain_per_frame_queues`.
    pub(super) fresh: bool,
    /// Drag latch. Sticky non-`None` for the press lifetime; doubles
    /// as "suppress click on release".
    pub(super) drag: PressDrag,
}

/// Drag latch of an in-flight [`Press`]: `None` until the pointer has
/// travelled [`DRAG_THRESHOLD`] from `origin`, `Started` on exactly
/// the threshold-crossing frame (the drag-start edge),
/// `Active` after — `drain_per_frame_queues` lowers the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PressDrag {
    #[default]
    None,
    Started,
    Active,
}

/// One-frame edge: how this button's capture ended this frame. One
/// value instead of three parallel edge fields — a click and a
/// drag-stop are mutually exclusive by construction, and either can
/// only target the widget that was released.
#[derive(Clone, Copy, Debug)]
pub(super) struct Release {
    /// The widget whose capture ended.
    pub(super) target: WidgetId,
    pub(super) kind: ReleaseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReleaseKind {
    /// The release landed back on the captured widget with no drag
    /// latched — a click. `count` is the press run's number
    /// (2 = double-click, 3 = triple…), stamped from [`Press::seq`].
    Click { count: u8 },
    /// A latched drag ended — the commit edge for drag gestures.
    DragStopped,
    /// Released off the widget with no drag latched — the capture
    /// just dissolves (drives the click-less `ButtonPhase::Up`).
    Miss,
}

/// Multi-press run state: where/when/on-what the last press landed and
/// its position in the run. The next press chains (`seq + 1`) when it
/// lands on the same `target` within [`DOUBLE_CLICK_WINDOW`] of `at`
/// and [`DOUBLE_CLICK_RADIUS`] of `pos`; any break restarts at 1.
#[derive(Clone, Copy, Debug)]
pub(super) struct PressRun {
    pub(super) at: Duration,
    pub(super) target: WidgetId,
    pub(super) pos: Vec2,
    pub(super) seq: u8,
}
