//! How far one wheel notch scrolls under the system's setting.

use glam::Vec2;

/// The system's scroll settings, read from the platform's file at each wheel event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WheelSteps {
    /// The lines one notch of the vertical wheel scrolls, where 0 scrolls nothing, as a person's
    /// setting can ask; `None` scrolls a page, the choice Windows calls "one screen at a time".
    pub(crate) vertical_lines: Option<f32>,
    /// No system states a page for the horizontal wheel.
    pub(crate) horizontal_lines: f32,
}

/// What a wheel turn scrolls, in each unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WheelTravel {
    pub(crate) lines: Vec2,
    pub(crate) pages: Vec2,
}

/// One line per notch, on a system that states no setting.
impl Default for WheelSteps {
    fn default() -> Self {
        Self {
            vertical_lines: Some(1.0),
            horizontal_lines: 1.0,
        }
    }
}

impl WheelSteps {
    /// What a turn of `notches` scrolls under these settings.
    pub(crate) const fn travel(self, notches: Vec2) -> WheelTravel {
        let lines_x = notches.x * self.horizontal_lines;
        match self.vertical_lines {
            Some(step) => WheelTravel {
                lines: Vec2::new(lines_x, notches.y * step),
                pages: Vec2::ZERO,
            },
            None => WheelTravel {
                lines: Vec2::new(lines_x, 0.0),
                pages: Vec2::new(0.0, notches.y),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::input::wheel_steps::{WheelSteps, WheelTravel};
    use glam::Vec2;

    /// Each setting takes the turn (2, −0.5) to its own travel: x × `horizontal_lines`, and y
    /// × the vertical step or into the page lane.
    #[test]
    fn a_turn_travels_by_each_setting() {
        let turn = Vec2::new(2.0, -0.5);
        let rows = [
            (WheelSteps::default(), [2.0, -0.5], [0.0, 0.0]),
            (
                WheelSteps {
                    vertical_lines: Some(3.0),
                    horizontal_lines: 4.0,
                },
                [8.0, -1.5],
                [0.0, 0.0],
            ),
            (
                WheelSteps {
                    vertical_lines: Some(0.0),
                    horizontal_lines: 0.0,
                },
                [0.0, 0.0],
                [0.0, 0.0],
            ),
            (
                WheelSteps {
                    vertical_lines: None,
                    horizontal_lines: 3.0,
                },
                [6.0, 0.0],
                [0.0, -0.5],
            ),
        ];
        for (steps, lines, pages) in rows {
            assert_eq!(
                steps.travel(turn),
                WheelTravel {
                    lines: Vec2::from(lines),
                    pages: Vec2::from(pages),
                },
                "{steps:?}",
            );
        }
    }
}
