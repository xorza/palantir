//! The rows a wheel turn reaches, one for each axis.

use crate::primitives::identity::widget_id::WidgetId;
use glam::Vec2;

/// The topmost row under the pointer that senses each wheel axis —
/// [`Sense::SCROLL_X`] and [`Sense::SCROLL_Y`] — which may be one row,
/// two, or none.
///
/// Per axis because a row senses only the axes it can pan, and the other
/// axis belongs to whatever pans it behind that row: the vertical wheel
/// over a one-line field inside a page scrolls the page, as browser
/// scroll chaining does.
///
/// [`Sense::SCROLL_X`]: crate::input::sense::Sense::SCROLL_X
/// [`Sense::SCROLL_Y`]: crate::input::sense::Sense::SCROLL_Y
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScrollTargets {
    pub(crate) x: Option<WidgetId>,
    pub(crate) y: Option<WidgetId>,
}

/// One row's share of a wheel event: the lanes of the delta it takes, the
/// other axis zeroed.
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

    /// Split one wheel event between the rows, at most one delivery per
    /// axis, and none for an axis the event does not move.
    ///
    /// A purely vertical event with no row to take it goes to the
    /// horizontal row instead, on its x lanes: a mouse wheel turns only
    /// vertically, and a tab strip or a one-line field pans only
    /// sideways. Only a *pure* one is moved, so a touchpad's diagonal
    /// swipe keeps its two axes apart.
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
        /// One row for both axes — what a `Sense::SCROLL` row resolves to
        /// with nothing above it.
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
