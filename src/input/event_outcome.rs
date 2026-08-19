//! What one handled input event asks of the frame.

/// What one event asks of the frame, as decided by the [`on_input`
/// arm](crate::input::input_state::InputState::on_input) that handled it. Both answers travel
/// together because every arm has to give both, and a side-effect
/// assignment for one of them made it easy to give only the other.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EventOutcome {
    /// The event could change what is on screen, so the next frame
    /// cannot stay on the paint-anim-only path. Surfaced to the host as
    /// [`InputDelta::requests_repaint`](crate::input::response::InputDelta::requests_repaint).
    pub(super) repaint: bool,
    /// The event wrote state that a widget recorded *earlier in the same
    /// pass* may already have read, so the pass has to run again.
    ///
    /// Set by: a `Click` or `DragStopped` release, a `KeyDown` or `Text`
    /// (both land in the keyboard queue), a drag latch crossing its
    /// threshold during a move, and any event a `PointerWake::BUTTONS`
    /// subscriber saw.
    ///
    /// Deliberately clear — though each still repaints: a **press**,
    /// because a capture reaches only its own target and `focused` is
    /// read live off `InputState`, committed before the frame, so pass A
    /// already sees it; a **`ReleaseKind::Miss`**, which fires no click
    /// and only tears down that same single-reader capture; and scroll,
    /// pinch, `PointerLeft` or modifier changes, whose state reaches
    /// exactly one routed target that applies it in the pass that
    /// receives it. An unrouted event leaves this clear because no
    /// widget or watcher can observe it.
    ///
    /// The press and `Miss` exclusions are what let a click-driven UI
    /// settle once per gesture instead of three times. The cost is
    /// narrow: an app that reacts to a press-driven focus change by
    /// writing state a *prefix* widget shows gains a one-frame lag, and
    /// should handle that edge in [`crate::App::update`] instead.
    pub(super) settles: bool,
}

impl EventOutcome {
    /// Repaints, but does not force a second record pass — the common
    /// case, and the one whose reasoning is on [`Self::settles`].
    #[inline]
    pub(super) fn repaint(repaint: bool) -> Self {
        Self {
            repaint,
            settles: false,
        }
    }

    /// Repaints and settles together. Every arm that settles also
    /// repaints, so no arm needs to state the two separately.
    #[inline]
    pub(super) fn settle(both: bool) -> Self {
        Self {
            repaint: both,
            settles: both,
        }
    }
}
