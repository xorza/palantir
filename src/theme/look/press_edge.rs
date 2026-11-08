//! [`PressEdge`]: the press a widget's response shows this frame, for its press effect.

use glam::Vec2;

use crate::primitives::input::interaction::button_phase::ButtonPhase;
use crate::primitives::input::interaction::response_state::ResponseState;

/// What the primary button does on a widget this frame, as its press effect follows it, points in
/// logical px from the widget's top-left. One case for each [`ButtonPhase`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PressEdge {
    /// No press is on the widget: one still held ends here, as when the widget was disabled under
    /// it, which empties its button state.
    Idle,
    /// A press landed at `point`.
    Pressed { point: Vec2 },
    /// The press goes on.
    Held,
    /// A press ended: the held one, or, with none held, one that pressed and released in this
    /// frame at `center`, as a focused widget's activation key gives it.
    Released { center: Vec2 },
}

impl PressEdge {
    /// The edge `state` shows.
    pub(crate) fn of(state: &ResponseState) -> Self {
        let center = state
            .layout_rect
            .map_or(Vec2::ZERO, |rect| Vec2::new(rect.size.w, rect.size.h) * 0.5);
        match state.left.phase {
            ButtonPhase::Idle => Self::Idle,
            ButtonPhase::Down { .. } => Self::Pressed {
                point: state.pointer_local.unwrap_or(center),
            },
            ButtonPhase::Held => Self::Held,
            ButtonPhase::Up { .. } => Self::Released { center },
        }
    }

    /// Whether this starts a press where none is live.
    pub(crate) const fn starts(self) -> bool {
        matches!(self, Self::Pressed { .. } | Self::Released { .. })
    }
}
