//! The rows a wheel turn reaches, one for each axis.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::scroll_delta::ScrollDelta;
use glam::Vec2;

/// The topmost row under the pointer that senses each wheel axis ([`Sense::SCROLL_X`], [`Sense::SCROLL_Y`]): one row, two, or none.
///
/// Per axis because a row senses only what it can pan; the other axis goes to whatever pans behind it (scroll chaining), so the vertical wheel over a one-line field scrolls the page.
///
/// [`Sense::SCROLL_X`]: crate::primitives::input::sense::Sense::SCROLL_X
/// [`Sense::SCROLL_Y`]: crate::primitives::input::sense::Sense::SCROLL_Y
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScrollTargets {
    pub(crate) x: Option<WidgetId>,
    pub(crate) y: Option<WidgetId>,
}

/// `f` of each lane of `delta`.
fn each_lane(delta: ScrollDelta, f: impl Fn(Vec2) -> Vec2) -> ScrollDelta {
    ScrollDelta {
        pixels: f(delta.pixels),
        lines: f(delta.lines),
        notches: f(delta.notches),
        pages: f(delta.pages),
        ..delta
    }
}

/// Whether `axis` of any lane of `delta` moves.
fn any_lane(delta: ScrollDelta, axis: impl Fn(Vec2) -> f32) -> bool {
    [delta.pixels, delta.lines, delta.notches, delta.pages]
        .into_iter()
        .any(|lane| axis(lane) != 0.0)
}

/// One row's share of a wheel event: its lanes of the delta, the other axis zeroed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WheelDelivery {
    pub(crate) target: WidgetId,
    pub(crate) delta: ScrollDelta,
}

impl ScrollTargets {
    /// Whether a row under the pointer senses either axis.
    pub(crate) const fn any(self) -> bool {
        self.x.is_some() || self.y.is_some()
    }

    /// Split one wheel event between the rows: at most one delivery per axis, none for an axis the event doesn't move. A purely vertical event with no vertical row goes to the horizontal row on its x lanes (a mouse wheel only turns vertically); a diagonal touchpad swipe keeps its axes apart.
    pub(crate) fn route(self, delta: ScrollDelta) -> [Option<WheelDelivery>; 2] {
        let moves_x = any_lane(delta, |lane| lane.x);
        let moves_y = any_lane(delta, |lane| lane.y);
        if !moves_x && moves_y && self.y.is_none() {
            let sideways = self.x.map(|target| WheelDelivery {
                target,
                delta: each_lane(delta, |lane| Vec2::new(lane.y, 0.0)),
            });
            return [sideways, None];
        }
        let x = self.x.filter(|_| moves_x).map(|target| WheelDelivery {
            target,
            delta: each_lane(delta, |lane| Vec2::new(lane.x, 0.0)),
        });
        let y = self.y.filter(|_| moves_y).map(|target| WheelDelivery {
            target,
            delta: each_lane(delta, |lane| Vec2::new(0.0, lane.y)),
        });
        [x, y]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::scroll_targets::ScrollTargets;

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
