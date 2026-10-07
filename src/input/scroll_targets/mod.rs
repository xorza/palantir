//! The rows a wheel turn reaches, one for each axis.

use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;

/// The topmost row under the pointer that senses each wheel axis ([`Sense::SCROLL_X`], [`Sense::SCROLL_Y`]): one row, two, or none.
///
/// Per axis because a row senses only what it can pan; the other axis goes to whatever pans behind it (scroll chaining), so the vertical wheel over a one-line field scrolls the page.
///
/// [`Sense::SCROLL_X`]: crate::input::sense::Sense::SCROLL_X
/// [`Sense::SCROLL_Y`]: crate::input::sense::Sense::SCROLL_Y
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScrollTargets {
    pub(crate) x: Option<WidgetId>,
    pub(crate) y: Option<WidgetId>,
}

/// One row's share of a wheel event: its lanes of the delta, the other axis zeroed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WheelDelivery {
    pub(crate) target: WidgetId,
    pub(crate) pixels: Vec2,
    pub(crate) lines: Vec2,
}

impl ScrollTargets {
    /// Whether a row under the pointer senses either axis.
    pub(crate) const fn any(self) -> bool {
        self.x.is_some() || self.y.is_some()
    }

    /// Split one wheel event between the rows: at most one delivery per axis, none for an axis the event doesn't move. A purely vertical event with no vertical row goes to the horizontal row on its x lanes (a mouse wheel only turns vertically); a diagonal touchpad swipe keeps its axes apart.
    pub(crate) fn route(self, pixels: Vec2, lines: Vec2) -> [Option<WheelDelivery>; 2] {
        let moves_x = pixels.x != 0.0 || lines.x != 0.0;
        let moves_y = pixels.y != 0.0 || lines.y != 0.0;
        if !moves_x && moves_y && self.y.is_none() {
            let sideways = self.x.map(|target| WheelDelivery {
                target,
                pixels: Vec2::new(pixels.y, 0.0),
                lines: Vec2::new(lines.y, 0.0),
            });
            return [sideways, None];
        }
        let x = self.x.filter(|_| moves_x).map(|target| WheelDelivery {
            target,
            pixels: Vec2::new(pixels.x, 0.0),
            lines: Vec2::new(lines.x, 0.0),
        });
        let y = self.y.filter(|_| moves_y).map(|target| WheelDelivery {
            target,
            pixels: Vec2::new(0.0, pixels.y),
            lines: Vec2::new(0.0, lines.y),
        });
        [x, y]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::scroll_targets::ScrollTargets;
    use crate::primitives::identity::widget_id::WidgetId;

    impl ScrollTargets {
        /// One row for both axes, as a `Sense::SCROLL` row resolves with nothing above it.
        pub(crate) const fn both(id: WidgetId) -> Self {
            Self {
                x: Some(id),
                y: Some(id),
            }
        }
    }
}

#[cfg(test)]
mod tests;
