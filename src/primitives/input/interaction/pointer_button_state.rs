//! One pointer button's slice of a widget's interaction snapshot.

use crate::primitives::input::interaction::drag::Drag;
use crate::primitives::input::interaction::pointer_button_phase::PointerButtonPhase;

/// A widget's interaction snapshot for one pointer button, so middle-click is as queryable as left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerButtonState {
    /// Press phase.
    pub phase: PointerButtonPhase,
    /// Drag lifecycle of this button's press (see [`Drag`]); each latched button reports its own.
    pub drag: Drag,
}

impl PointerButtonState {
    /// Not down on the widget, and no drag: what a button reports with nothing happening.
    pub const IDLE: Self = Self {
        phase: PointerButtonPhase::Idle,
        drag: Drag::None,
    };

    /// Pairs a phase with its drag, the one place the pairing rule lives: a live drag implies a live press, a
    /// stopped one the click-less release that ended it. Every router value is built here, so impossible
    /// combinations are caught where introduced.
    #[inline]
    pub(crate) fn new(phase: PointerButtonPhase, drag: Drag) -> Self {
        debug_assert!(
            match drag {
                Drag::None => true,
                Drag::Started { .. } | Drag::Active { .. } => matches!(
                    phase,
                    PointerButtonPhase::Down { .. } | PointerButtonPhase::Held
                ),
                Drag::Stopped => phase == PointerButtonPhase::Up { click: None },
            },
            "{phase:?} cannot be driving {drag:?}",
        );
        Self { phase, drag }
    }

    /// Whether the button is down.
    #[inline]
    pub const fn held(self) -> bool {
        matches!(
            self.phase,
            PointerButtonPhase::Down { .. } | PointerButtonPhase::Held
        )
    }

    /// One-frame edge: the press landed this frame, [`PointerButtonPhase::Down`]; the edge
    /// [`Self::released`] closes.
    #[inline]
    pub const fn pressed(self) -> bool {
        matches!(self.phase, PointerButtonPhase::Down { .. })
    }

    /// One-frame edge: a press+release landed on the widget without latching a drag.
    #[inline]
    pub const fn clicked(self) -> bool {
        matches!(self.phase, PointerButtonPhase::Up { click: Some(_) })
    }

    /// One-frame edge: the press ended this frame (click or latched-drag release); where `committed` reports.
    #[inline]
    pub const fn released(self) -> bool {
        matches!(self.phase, PointerButtonPhase::Up { .. })
    }

    /// Consecutive presses while down; `0` otherwise.
    #[inline]
    pub const fn press_count(self) -> u8 {
        match self.phase {
            PointerButtonPhase::Down { count } => count,
            _ => 0,
        }
    }

    /// This frame's click-run position: `0` off the click edge, else 1/2/3+.
    #[inline]
    pub const fn click_count(self) -> u8 {
        match self.phase {
            PointerButtonPhase::Up { click: Some(n) } => n,
            _ => 0,
        }
    }

    /// One-frame edge: this click was the second in its run (`click_count() == 2`).
    #[inline]
    pub const fn double_clicked(self) -> bool {
        self.click_count() == 2
    }
}

/// [`PointerButtonState::IDLE`], the one value nothing happening reads as.
impl Default for PointerButtonState {
    fn default() -> Self {
        Self::IDLE
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::input::interaction::drag::Drag;
    use crate::primitives::input::interaction::pointer_button_phase::PointerButtonPhase;
    use crate::primitives::input::interaction::pointer_button_state::PointerButtonState;

    /// Each phase answers every read: `held` is the level from the press edge through the
    /// frames it latches, `pressed` and `released` the two edges, a click the release that
    /// completed one, with its position in the run, and the press its count.
    #[test]
    fn each_phase_answers_its_edges_and_counts() {
        use PointerButtonPhase::{Down, Held, Idle, Up};
        // (phase, held, pressed, released, clicked, press_count, click_count, double_clicked)
        let cases = [
            (Idle, false, false, false, false, 0, 0, false),
            (Down { count: 2 }, true, true, false, false, 2, 0, false),
            (Held, true, false, false, false, 0, 0, false),
            (Up { click: None }, false, false, true, false, 0, 0, false),
            (Up { click: Some(1) }, false, false, true, true, 0, 1, false),
            (Up { click: Some(2) }, false, false, true, true, 0, 2, true),
        ];
        for (phase, held, pressed, released, clicked, presses, clicks, double) in cases {
            let state = PointerButtonState::new(phase, Drag::None);
            assert_eq!(
                (
                    state.held(),
                    state.pressed(),
                    state.released(),
                    state.clicked(),
                    state.press_count(),
                    state.click_count(),
                    state.double_clicked(),
                ),
                (held, pressed, released, clicked, presses, clicks, double),
                "{phase:?}"
            );
        }
        assert_eq!(PointerButtonState::default(), PointerButtonState::IDLE);
        assert_eq!(PointerButtonState::IDLE.phase, Idle);
        assert_eq!(PointerButtonState::IDLE.drag, Drag::None);
    }
}
