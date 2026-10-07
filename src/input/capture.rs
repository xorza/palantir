//! The per-button press-capture state machine: what a press latched onto,
//! whether it became a drag, how it ended, and how presses chain into
//! double- and triple-click runs.
//!
//! Kept together because the invariants hold across the set: a capture always
//! has a press origin, a drag latch always has a capture, click and drag-stop
//! never coexist, and the run tracker never half-exists.

use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;
use std::time::Duration;

/// Pointer travel (logical px) from the press origin before a gesture latches
/// as a drag. The latch holds for the press and the release emits no click.
pub(crate) const DRAG_THRESHOLD: f32 = 4.0;

/// Maximum interval between two clicks on the same widget for a double-click.
/// 500 ms matches the Windows / Chromium default; Linux has no system value.
pub(crate) const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// Maximum pointer travel (logical px) between two clicks for a double-click.
/// [`TextEdit`](crate::TextEdit)'s word and all-selection read the run it
/// bounds rather than keeping a radius of their own.
pub(super) const DOUBLE_CLICK_RADIUS: f32 = 5.0;

/// Per-button capture, one slot per
/// [`PointerButton`](crate::input::pointer::PointerButton). Three
/// all-or-nothing pieces make the invariants in the module doc
/// unrepresentable to break.
#[derive(Default, Clone, Copy, Debug)]
pub(super) struct Capture {
    /// The in-flight press; `Some` means the capture is latched.
    pub(super) press: Option<Press>,
    /// One-frame edge: how a capture ended this frame. Cleared by `end_frame`.
    pub(super) release: Option<Release>,
    /// Multi-press run tracker. Persists across presses; only replaced by the
    /// next press.
    pub(super) run: Option<PressRun>,
}

impl Capture {
    /// Latch a press on `target` at `pos`, chaining the run when it lands on
    /// the same target within [`DOUBLE_CLICK_WINDOW`] and
    /// [`DOUBLE_CLICK_RADIUS`] of the previous press; any break restarts at
    /// 1. `count` saturates instead of wrapping.
    pub(super) fn begin_press(&mut self, target: WidgetId, pos: Vec2, now: Duration) {
        let count = match &self.run {
            Some(run)
                if run.target == target
                    && now.saturating_sub(run.at) <= DOUBLE_CLICK_WINDOW
                    && pos.distance(run.pos) <= DOUBLE_CLICK_RADIUS =>
            {
                run.count.saturating_add(1)
            }
            _ => 1,
        };
        self.run = Some(PressRun {
            at: now,
            target,
            pos,
            count,
        });
        self.press = Some(Press {
            target,
            origin: pos,
            travel: Vec2::ZERO,
            count,
            fresh: true,
            drag: PressDrag::None,
        });
    }

    /// End the in-flight press and record how.
    ///
    /// The only way a press leaves a capture. Every end writes a [`Release`],
    /// which both downstream collations read (a widget's `ButtonPhase` /
    /// [`Drag`](crate::Drag) and `pointer_actions`'s
    /// [`PointerEdge`](crate::PointerEdge)); without one, a widget committing
    /// on `drag.stopped()` never commits.
    pub(super) fn end_press(&mut self, kind: impl FnOnce(&Press) -> ReleaseKind) {
        let Some(press) = self.press.take() else {
            return;
        };
        self.release = Some(Release {
            target: press.target,
            kind: kind(&press),
        });
    }

    /// End the press because the gesture was cut off (the widget left the
    /// tree, or the surface lost focus). A latched drag still owes its commit
    /// edge; an unlatched press dissolves as [`ReleaseKind::Miss`].
    pub(super) fn abandon_press(&mut self) {
        self.end_press(|press| {
            if press.drag == PressDrag::None {
                ReleaseKind::Miss
            } else {
                ReleaseKind::DragStopped
            }
        });
    }
}

/// One in-flight press: capture target, drag anchor and run position.
#[derive(Clone, Copy, Debug)]
pub(super) struct Press {
    /// Widget the press latched onto.
    pub(super) target: WidgetId,
    /// Pointer position at the press; the anchor for [`Self::travel`].
    pub(super) origin: Vec2,
    /// Surface-space travel since [`Self::origin`], refreshed on every
    /// pointer move.
    ///
    /// Retained rather than read from the live pointer: a drag survives the
    /// pointer leaving the window, where the live pointer is `None` and would
    /// stop the drag being reported while the latch (read by
    /// `pointer_actions`) is still set. The reader applies its own widget's
    /// transform.
    pub(super) travel: Vec2,
    /// Position in the multi-press run (1 = single, 2 = double, ...), stamped
    /// at press time so the release does not depend on later run state.
    pub(super) count: u8,
    /// One-frame edge: the press landed this frame (`ButtonPhase::Down`).
    /// Lowered by `drain_per_frame_queues`.
    pub(super) fresh: bool,
    /// Drag latch, sticky for the press; also suppresses click on release.
    pub(super) drag: PressDrag,
}

/// Drag latch of a [`Press`]: `None` until travel exceeds [`DRAG_THRESHOLD`],
/// `Started` on that frame, `Active` after (`drain_per_frame_queues` lowers
/// the edge).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PressDrag {
    #[default]
    None,
    Started,
    Active,
}

/// One-frame edge: how this button's capture ended. One value, so a click and
/// a drag-stop cannot coexist.
#[derive(Clone, Copy, Debug)]
pub(super) struct Release {
    /// The widget whose capture ended.
    pub(super) target: WidgetId,
    pub(super) kind: ReleaseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReleaseKind {
    /// Released back on the captured widget with no drag latched. `count` is
    /// the press run's number, stamped from [`Press::count`].
    Click { count: u8 },
    /// A latched drag ended: the commit edge.
    DragStopped,
    /// Released off the widget with no drag latched (`ButtonPhase::Up`).
    Miss,
}

impl ReleaseKind {
    /// The click this release completed and its place in the press run;
    /// `None` when a drag ate it or it landed off the widget. Both collations
    /// read it so they cannot disagree about what a click was.
    pub(super) const fn click(self) -> Option<u8> {
        match self {
            Self::Click { count } => Some(count),
            Self::DragStopped | Self::Miss => None,
        }
    }

    /// Whether this release ended a latched drag, read by both collations.
    pub(super) fn ended_drag(self) -> bool {
        self == Self::DragStopped
    }
}

/// Multi-press run state: where, when and on what the last press landed. The
/// next press chains (`count + 1`) on the same `target` within
/// [`DOUBLE_CLICK_WINDOW`] and [`DOUBLE_CLICK_RADIUS`]; any break restarts at 1.
#[derive(Clone, Copy, Debug)]
pub(super) struct PressRun {
    pub(super) at: Duration,
    pub(super) target: WidgetId,
    pub(super) pos: Vec2,
    pub(super) count: u8,
}
