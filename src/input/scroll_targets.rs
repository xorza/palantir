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
mod tests {
    use crate::input::scroll_targets::{ScrollTargets, WheelDelivery};
    use crate::primitives::identity::widget_id::WidgetId;
    use glam::Vec2;

    /// Every split the routing makes, against deliveries written out by
    /// hand: the axes apart, one row for both, the sideways move and the
    /// cases that must not make it.
    #[test]
    fn route_splits_by_axis_and_moves_only_a_pure_vertical_turn() {
        type Case = (
            &'static str,
            ScrollTargets,
            [f32; 2],
            [f32; 2],
            [Option<WheelDelivery>; 2],
        );
        let row = WidgetId::from_hash("row");
        let page = WidgetId::from_hash("page");
        let at = |target, pixels: [f32; 2], lines: [f32; 2]| {
            Some(WheelDelivery {
                target,
                pixels: Vec2::from(pixels),
                lines: Vec2::from(lines),
            })
        };
        let row_x = ScrollTargets {
            x: Some(row),
            y: None,
        };
        let split = ScrollTargets {
            x: Some(row),
            y: Some(page),
        };
        let cases: [Case; 8] = [
            (
                "diagonal pixels, two rows",
                split,
                [3.0, 4.0],
                [0.0, 0.0],
                [
                    at(row, [3.0, 0.0], [0.0, 0.0]),
                    at(page, [0.0, 4.0], [0.0, 0.0]),
                ],
            ),
            (
                "a vertical notch skips the row that pans only x",
                split,
                [0.0, 0.0],
                [0.0, 2.0],
                [None, at(page, [0.0, 0.0], [0.0, 2.0])],
            ),
            (
                "one row for both axes takes the halves that sum to the event",
                ScrollTargets::both(row),
                [3.0, 4.0],
                [1.0, 2.0],
                [
                    at(row, [3.0, 0.0], [1.0, 0.0]),
                    at(row, [0.0, 4.0], [0.0, 2.0]),
                ],
            ),
            (
                "a pure vertical turn with no y row pans the x row",
                row_x,
                [0.0, 5.0],
                [0.0, 2.0],
                [at(row, [5.0, 0.0], [2.0, 0.0]), None],
            ),
            (
                "a horizontal turn reaches the x row as it came",
                row_x,
                [5.0, 0.0],
                [0.0, 0.0],
                [at(row, [5.0, 0.0], [0.0, 0.0]), None],
            ),
            (
                "a diagonal swipe keeps its y apart, and nothing takes it",
                row_x,
                [1.0, 5.0],
                [0.0, 0.0],
                [at(row, [1.0, 0.0], [0.0, 0.0]), None],
            ),
            (
                "a horizontal turn does not move to a y-only row",
                ScrollTargets {
                    x: None,
                    y: Some(page),
                },
                [5.0, 0.0],
                [0.0, 0.0],
                [None, None],
            ),
            (
                "an empty event delivers nothing",
                split,
                [0.0, 0.0],
                [0.0, 0.0],
                [None, None],
            ),
        ];
        for (label, targets, pixels, lines, expected) in cases {
            assert_eq!(
                targets.route(Vec2::from(pixels), Vec2::from(lines)),
                expected,
                "{label}",
            );
        }
        assert!(row_x.any());
        assert!(!ScrollTargets::default().any());
    }
}
