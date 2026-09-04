//! One pointer button's slice of a widget's interaction snapshot: its
//! phase, its edges, and the drag it may be driving.

use crate::input::response::button_phase::ButtonPhase;
use crate::input::response::drag::Drag;

/// One pointer button's slice of a widget's interaction snapshot.
/// [`ResponseState`](crate::ResponseState) carries one per
/// [`PointerButton`](crate::PointerButton) — every button
/// gets the same uniform surface (middle-click is as queryable as
/// left).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonState {
    /// Press lifecycle (see [`ButtonPhase`]).
    pub phase: ButtonPhase,
    /// Drag lifecycle (see [`Drag`]). At most one button's drag is
    /// live per widget: when several buttons are simultaneously
    /// latched, the first in [`PointerButton`](crate::PointerButton)'s
    /// declaration order wins.
    pub drag: Drag,
}

impl ButtonState {
    /// Pair a phase with the drag it is driving.
    ///
    /// **The one constructor**, and the one place the pairing rule is
    /// written: a live drag implies a live press, and a stopped one
    /// implies the click-less release that ended it. The struct's fields
    /// stay public because this is a snapshot a widget reads, never one
    /// it hands back — but every value the router produces is built here,
    /// so a combination the router cannot mean is caught where it would
    /// be introduced.
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

    /// The press is latched on the widget (`Down` or `Held`) —
    /// rect-independent, no travel threshold.
    #[inline]
    pub fn held(self) -> bool {
        matches!(self.phase, ButtonPhase::Down { .. } | ButtonPhase::Held)
    }

    /// One-frame edge: a press+release landed on the widget without
    /// latching a drag. Fires on the release. For double/triple
    /// dispatch read [`Self::click_count`] (`== 2` is the
    /// double-click).
    #[inline]
    pub fn clicked(self) -> bool {
        matches!(self.phase, ButtonPhase::Up { click: Some(_) })
    }

    /// One-frame edge: the press ended this frame, whether as a click or
    /// as the release of a latched drag. The frame a value-writing widget
    /// reports `committed` on — a gesture is one edit however it ended.
    #[inline]
    pub fn released(self) -> bool {
        matches!(self.phase, ButtonPhase::Up { .. })
    }

    /// This frame's press-run position: `0` off the press edge,
    /// 1/2/3+ on it (`press_count() > 0` is the press-rising edge).
    #[inline]
    pub fn press_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Down { count } => count,
            _ => 0,
        }
    }

    /// This frame's click-run position: `0` off the click edge,
    /// 1/2/3+ on it (`2` = double-click, `3` = triple-click).
    #[inline]
    pub fn click_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Up { click } => click.unwrap_or(0),
            _ => 0,
        }
    }

    /// One-frame edge: this click completed a double (its press was
    /// the second in its run). Sugar for `click_count() == 2` — read
    /// [`Self::click_count`] for triple and beyond.
    #[inline]
    pub fn double_clicked(self) -> bool {
        self.click_count() == 2
    }
}
