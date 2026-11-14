//! The per-button press-capture state machine: what a press latched onto,
//! whether it became a drag, how it ended, and how presses chain into
//! double- and triple-click runs.
//!
//! Kept together because the invariants hold across the set: a capture always
//! has a press origin, a drag latch always has a capture, click and drag-stop
//! never coexist, and the run tracker never half-exists.

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::drag::Drag;
use crate::primitives::input::interaction::pointer_action::PointerAction;
use crate::primitives::input::interaction::pointer_button_phase::PointerButtonPhase;
use crate::primitives::input::interaction::pointer_button_state::PointerButtonState;
use crate::primitives::input::interaction::pointer_edge::PointerEdge;
use crate::primitives::input::pointer::PointerButton;
use glam::Vec2;
use std::time::Duration;

/// Pointer travel (logical px) from the press origin before a gesture latches
/// as a drag. The latch holds for the press and the release emits no click.
pub(crate) const DRAG_THRESHOLD: f32 = 4.0;

/// Maximum interval between two clicks on the same widget for a double-click when no platform
/// setting is read: the Windows and macOS default, and what a host with no desktop keeps, so a
/// test repeats. The winit host reads the platform's own setting in its place.
pub const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);

/// Maximum pointer travel (logical px) between two clicks for a double-click. Held fixed: macOS and
/// Wayland have no setting for it, Windows a rectangle in physical pixels (`SM_CXDOUBLECLK`), and
/// X11's XSETTINGS `Net/DoubleClickDistance`.
/// [`TextEdit`](crate::TextEdit)'s word and all-selection read the run it
/// bounds rather than keeping a radius of their own.
pub const DOUBLE_CLICK_RADIUS: f32 = 5.0;

/// Per-button capture, one slot per
/// [`PointerButton`]. Three
/// all-or-nothing pieces make the invariants in the module doc
/// unrepresentable to break.
#[derive(Default, Clone, Copy, Debug)]
pub(super) struct Capture {
    /// The in-flight press; `Some` means the capture is latched.
    press: Option<Press>,
    /// One-frame edge: how a capture ended this frame. Cleared by [`Self::end_frame`].
    release: Option<Release>,
    /// Multi-press run tracker. Persists across presses; only replaced by the
    /// next press.
    run: Option<PressRun>,
}

impl Capture {
    /// Latch a press on `target` at `pos`, chaining the run when it lands on
    /// the same target within `interval` and [`DOUBLE_CLICK_RADIUS`] of the
    /// previous press; any break restarts at 1. `count` saturates instead of
    /// wrapping.
    pub(super) fn begin_press(
        &mut self,
        target: WidgetId,
        pos: Vec2,
        now: Duration,
        interval: Duration,
    ) {
        let count = match &self.run {
            Some(run)
                if run.target == target
                    && now.saturating_sub(run.at) <= interval
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
    /// which both downstream collations read (a widget's `PointerButtonPhase` /
    /// [`Drag`] and `pointer_actions`'s
    /// [`PointerEdge`]); without one, a widget committing
    /// on `drag.stopped()` never commits.
    fn end_press(&mut self, kind: impl FnOnce(&Press) -> ReleaseKind) {
        let Some(press) = self.press.take() else {
            return;
        };
        self.release = Some(Release {
            target: press.target,
            kind: kind(&press),
        });
    }

    /// End the press as `pointer` lands its release, where the hit test `hit` finds the
    /// clickable widget under it: a latched drag's stop, else a click back on the widget,
    /// carrying its press's run number, else a miss. `None` when no press was held.
    pub(super) fn release_on(
        &mut self,
        hit: impl FnOnce() -> Option<WidgetId>,
    ) -> Option<ReleaseKind> {
        self.press?;
        self.end_press(|press| {
            if press.drag != PressDrag::None {
                ReleaseKind::DragStopped
            } else if hit() == Some(press.target) {
                ReleaseKind::Click { count: press.count }
            } else {
                ReleaseKind::Miss
            }
        });
        self.release.map(|release| release.kind)
    }

    /// Carry a held press's travel to `pos`; a press whose travel crosses [`DRAG_THRESHOLD`]
    /// latches a drag and ends its multi-click run. Returns whether it latched now.
    pub(super) fn travel_to(&mut self, pos: Vec2) -> bool {
        let Some(press) = &mut self.press else {
            return false;
        };
        press.travel = pos - press.origin;
        if press.drag != PressDrag::None
            || press.travel.length_squared() < DRAG_THRESHOLD * DRAG_THRESHOLD
        {
            return false;
        }
        press.drag = PressDrag::Started;
        self.run = None;
        true
    }

    /// End the multi-click run, so the next press starts one at 1.
    pub(super) const fn break_run(&mut self) {
        self.run = None;
    }

    /// Forget this frame's edges: the release, a fresh press, a drag that started.
    pub(super) const fn end_frame(&mut self) {
        self.release = None;
        if let Some(press) = &mut self.press {
            press.fresh = false;
            if matches!(press.drag, PressDrag::Started) {
                press.drag = PressDrag::Active;
            }
        }
    }

    /// Whether a press is held.
    pub(super) const fn is_pressed(&self) -> bool {
        self.press.is_some()
    }

    /// The widget a held press latched onto.
    pub(super) fn pressed_on(&self) -> Option<WidgetId> {
        self.press.map(|press| press.target)
    }

    /// Whether the capture ended this frame, an edge the frame still owes.
    pub(super) const fn has_edge(&self) -> bool {
        self.release.is_some()
    }

    /// This frame's edges of `button`'s capture, each on the widget it lands on: a fresh press,
    /// a drag that started, and how the capture ended. A release destroys the press and
    /// `InputQueue` holds a new press for the next frame, so never a press and an end both.
    pub(super) fn edges(&self, button: PointerButton) -> impl Iterator<Item = PointerAction> {
        let of = move |id, edge| PointerAction { id, button, edge };
        let pressed = self
            .press
            .filter(|press| press.fresh)
            .map(|press| of(press.target, PointerEdge::Pressed { count: press.count }));
        let dragging = self
            .press
            .filter(|press| press.drag == PressDrag::Started)
            .map(|press| of(press.target, PointerEdge::DragStarted));
        let ended = self.release.and_then(|release| {
            let edge = match release.kind.click() {
                Some(count) => PointerEdge::Clicked { count },
                None if release.kind.ended_drag() => PointerEdge::DragStopped,
                None => return None,
            };
            Some(of(release.target, edge))
        });
        [pressed, dragging, ended].into_iter().flatten()
    }

    /// What `id` reads of this button: a live press is `Down` while fresh, `Held` after, else a
    /// release edge is `Up`; a latched drag carries its travel in `id`'s frame through
    /// `transform`, else a release that ended one is its stop. Read off the press, not the live
    /// pointer, so it agrees with [`Self::edges`]: leaving the window mid-drag does not end the
    /// gesture.
    pub(super) fn button_state(
        &self,
        id: WidgetId,
        transform: TranslateScale,
    ) -> PointerButtonState {
        let press = self.press.filter(|press| press.target == id);
        let release = self.release.filter(|release| release.target == id);
        let phase = match (press, release) {
            (Some(press), _) if press.fresh => PointerButtonPhase::Down { count: press.count },
            (Some(_), _) => PointerButtonPhase::Held,
            (None, Some(release)) => PointerButtonPhase::Up {
                click: release.kind.click(),
            },
            (None, None) => PointerButtonPhase::Idle,
        };
        let drag = match (press, release) {
            (Some(press), _) if press.drag != PressDrag::None => {
                let delta = transform.inverse_vector(press.travel);
                if press.drag == PressDrag::Started {
                    Drag::Started { delta }
                } else {
                    Drag::Active { delta }
                }
            }
            (Some(_), _) => Drag::None,
            (None, Some(release)) if release.kind.ended_drag() => Drag::Stopped,
            (None, _) => Drag::None,
        };
        PointerButtonState::new(phase, drag)
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
struct Press {
    /// Widget the press latched onto.
    target: WidgetId,
    /// Pointer position at the press; the anchor for [`Self::travel`].
    origin: Vec2,
    /// Surface-space travel since [`Self::origin`], refreshed on every
    /// pointer move.
    ///
    /// Retained rather than read from the live pointer: a drag survives the
    /// pointer leaving the window, where the live pointer is `None` and would
    /// stop the drag being reported while the latch (read by
    /// `pointer_actions`) is still set. The reader applies its own widget's
    /// transform.
    travel: Vec2,
    /// Position in the multi-press run (1 = single, 2 = double, ...), stamped
    /// at press time so the release does not depend on later run state.
    count: u8,
    /// One-frame edge: the press landed this frame (`PointerButtonPhase::Down`).
    /// Lowered by `Input::clear_edges`.
    fresh: bool,
    /// Drag latch, sticky for the press; also suppresses click on release.
    drag: PressDrag,
}

/// Drag latch of a [`Press`]: `None` until travel exceeds [`DRAG_THRESHOLD`],
/// `Started` on that frame, `Active` after (`Input::clear_edges` lowers
/// the edge).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PressDrag {
    #[default]
    None,
    Started,
    Active,
}

/// One-frame edge: how this button's capture ended. One value, so a click and
/// a drag-stop cannot coexist.
#[derive(Clone, Copy, Debug)]
struct Release {
    /// The widget whose capture ended.
    target: WidgetId,
    kind: ReleaseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReleaseKind {
    /// Released back on the captured widget with no drag latched. `count` is
    /// the press run's number, stamped from [`Press::count`].
    Click { count: u8 },
    /// A latched drag ended: the commit edge.
    DragStopped,
    /// Released off the widget with no drag latched (`PointerButtonPhase::Up`).
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
    pub(super) const fn ended_drag(self) -> bool {
        matches!(self, Self::DragStopped)
    }
}

/// Multi-press run state: where, when and on what the last press landed. The
/// next press chains (`count + 1`) on the same `target` within
/// the double-click interval and [`DOUBLE_CLICK_RADIUS`]; any break restarts at 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PressRun {
    at: Duration,
    target: WidgetId,
    pos: Vec2,
    count: u8,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::capture::{Capture, PressRun};
    use crate::primitives::identity::widget_id::WidgetId;
    use glam::Vec2;
    use std::time::Duration;

    impl PressRun {
        pub(crate) const fn new(at: Duration, target: WidgetId, pos: Vec2, count: u8) -> Self {
            Self {
                at,
                target,
                pos,
                count,
            }
        }
    }

    impl Capture {
        /// A capture after `run`, no press held.
        pub(crate) fn after(run: PressRun) -> Self {
            Self {
                run: Some(run),
                ..Self::default()
            }
        }

        /// The held press's place in its run.
        pub(crate) fn press_count(&self) -> Option<u8> {
            self.press.map(|press| press.count)
        }

        /// The run the last press left.
        pub(crate) const fn run(&self) -> Option<PressRun> {
            self.run
        }
    }
}
