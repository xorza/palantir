//! What one handled input event asks of the frame.

/// What one event asks of the frame, decided by the [`on_input`
/// arm](crate::input::input_state::InputState::on_input) that handled it; every arm must give both answers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EventOutcome {
    /// The event could change what is on screen (surfaced as `InputDelta::repaint_requested`).
    pub(super) repaint: bool,
    /// The event wrote state that a widget recorded *earlier in the same pass* may already have read, so the pass runs again.
    ///
    /// Set by a `Click` or `DragStopped` release, a `KeyDown`, a drag latch crossing its threshold, and any event a
    /// `PointerWake::BUTTONS` subscriber saw. Clear for a press (a capture reaches only its own target and `focused` is
    /// read live), `ReleaseKind::Miss`, scroll, pinch, `PointerLeft`, modifier changes and unrouted events: they reach
    /// at most one target that applies them in the pass that receives them. That lets a click-driven UI settle once
    /// per gesture; an app reacting to a press-driven focus change by writing state a *prefix* widget shows gains a
    /// one-frame lag and should handle it in [`crate::App::update`].
    pub(super) settles: bool,
}

impl EventOutcome {
    /// Repaints without forcing a second record pass: the common case.
    #[inline]
    pub(super) const fn repaint(repaint: bool) -> Self {
        Self {
            repaint,
            settles: false,
        }
    }

    /// Repaints and settles together; every settling arm also repaints.
    #[inline]
    pub(super) const fn settle(both: bool) -> Self {
        Self {
            repaint: both,
            settles: both,
        }
    }
}
