//! [`WheelDelta`]: one wheel or touchpad turn, or a frame's sum of them, in each of its units.

use std::ops::{Add, AddAssign};

use glam::Vec2;

/// A wheel or touchpad turn in each unit it scrolls by, each lane negated at ingest so `+y`
/// advances a scroll offset. A notched turn keeps its notches beside what the system's setting
/// makes of them, its lines or its pages, as a step per notch, a wheel zoom, reads the notches
/// alone: a setting that multiplies a scroll must not multiply a zoom
/// ([GTK #1408](https://gitlab.gnome.org/GNOME/gtk/issues/1408)). Turns add lane by lane, so one
/// value holds a turn, the queue's merge of two and a frame's sum alike.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelDelta {
    /// Pixel-precise travel in logical pixels, as touchpads and precision wheels report it.
    pub pixels: Vec2,
    /// Notched travel in lines under the system's scroll setting.
    pub lines: Vec2,
    /// How far a notched wheel turned, whatever the system's setting makes of it.
    pub notches: Vec2,
    /// Notched travel in pages, where the system's setting is a page per notch.
    pub pages: Vec2,
}

impl WheelDelta {
    /// No travel.
    pub const ZERO: Self = Self {
        pixels: Vec2::ZERO,
        lines: Vec2::ZERO,
        notches: Vec2::ZERO,
        pages: Vec2::ZERO,
    };

    /// Pixel-precise travel alone.
    pub const fn pixels(pixels: Vec2) -> Self {
        Self {
            pixels,
            ..Self::ZERO
        }
    }

    /// A turn of `notches` at one line a notch, where no system's setting applies.
    pub const fn notches(notches: Vec2) -> Self {
        Self {
            lines: notches,
            notches,
            ..Self::ZERO
        }
    }

    /// Whether every lane is finite.
    pub(crate) fn is_finite(self) -> bool {
        self.pixels.is_finite()
            && self.lines.is_finite()
            && self.notches.is_finite()
            && self.pages.is_finite()
    }
}

impl Add for WheelDelta {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            pixels: self.pixels + other.pixels,
            lines: self.lines + other.lines,
            notches: self.notches + other.notches,
            pages: self.pages + other.pages,
        }
    }
}

impl AddAssign for WheelDelta {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::input::interaction::wheel_delta::WheelDelta;
    use glam::Vec2;

    /// A notched turn scrolls a line a notch; two turns add lane by lane.
    #[test]
    fn turns_add_lane_by_lane() {
        let turn = WheelDelta::notches(Vec2::new(0.0, 2.0));
        assert_eq!(
            turn,
            WheelDelta {
                lines: Vec2::new(0.0, 2.0),
                notches: Vec2::new(0.0, 2.0),
                ..WheelDelta::ZERO
            }
        );
        let mut sum = turn + WheelDelta::pixels(Vec2::new(3.0, -1.0));
        sum += WheelDelta {
            pages: Vec2::new(0.0, 1.0),
            ..WheelDelta::ZERO
        };
        assert_eq!(
            sum,
            WheelDelta {
                pixels: Vec2::new(3.0, -1.0),
                lines: Vec2::new(0.0, 2.0),
                notches: Vec2::new(0.0, 2.0),
                pages: Vec2::new(0.0, 1.0),
            }
        );
        assert!(sum.is_finite());
        assert!(!WheelDelta::pixels(Vec2::new(f32::NAN, 0.0)).is_finite());
    }
}
