//! One pointer button's slice of a widget's interaction snapshot.

use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::interaction::drag::Drag;

/// A widget's interaction snapshot for one pointer button, so middle-click is as queryable as left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonState {
    /// Press phase.
    pub phase: ButtonPhase,
    /// Drag lifecycle (see [`Drag`]); with several buttons latched, the first in declaration order wins.
    pub drag: Drag,
}

impl ButtonState {
    /// Pairs a phase with its drag, the one place the pairing rule lives: a live drag implies a live press, a
    /// stopped one the click-less release that ended it. Every router value is built here, so impossible
    /// combinations are caught where introduced.
    #[inline]
    pub(crate) fn new(phase: ButtonPhase, drag: Drag) -> Self {
        debug_assert!(
            match drag {
                Drag::None => true,
                Drag::Started { .. } | Drag::Active { .. } =>
                    matches!(phase, ButtonPhase::Down { .. } | ButtonPhase::Held),
                Drag::Stopped => phase == ButtonPhase::Up { click: None },
            },
            "{phase:?} cannot be driving {drag:?}",
        );
        Self { phase, drag }
    }

    #[inline]
    /// Whether the button is down.
    pub const fn held(self) -> bool {
        matches!(self.phase, ButtonPhase::Down { .. } | ButtonPhase::Held)
    }

    /// One-frame edge: a press+release landed on the widget without latching a drag.
    #[inline]
    pub const fn clicked(self) -> bool {
        matches!(self.phase, ButtonPhase::Up { click: Some(_) })
    }

    /// One-frame edge: the press ended this frame (click or latched-drag release); where `committed` reports.
    #[inline]
    pub const fn released(self) -> bool {
        matches!(self.phase, ButtonPhase::Up { .. })
    }

    #[inline]
    /// Consecutive presses while down; `0` otherwise.
    pub const fn press_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Down { count } => count,
            _ => 0,
        }
    }

    /// This frame's click-run position: `0` off the click edge, else 1/2/3+.
    #[inline]
    pub const fn click_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Up { click: Some(n) } => n,
            _ => 0,
        }
    }

    /// One-frame edge: this click was the second in its run (`click_count() == 2`).
    #[inline]
    pub const fn double_clicked(self) -> bool {
        self.click_count() == 2
    }
}
