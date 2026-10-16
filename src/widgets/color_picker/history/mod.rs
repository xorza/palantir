//! The swatch row a picker keeps for itself, and the colors it starts with.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::okhsv::Okhsv;
use tinyvec::ArrayVec;

/// Recently committed colors, most recent first, seeded with a preset row so it keeps its length; the newest
/// goes to the front and the oldest preset falls off. Seeded on first use, not at construction: picker state is
/// taken out and put back every frame, so a default that built the presets would pay sixteen conversions a frame.
#[derive(Debug, Default)]
pub(crate) struct History {
    colors: ArrayVec<[RgbaF32; Self::CAP]>,
}

impl History {
    const CAP: usize = 16;

    /// Evenly spaced Okhsv hues in the row, derived rather than hand-picked.
    const HUES: usize = 12;

    const NEUTRALS: [f32; 4] = [0.0, 0.35, 0.7, 1.0];

    /// The preset row, computed when a picker first opens its history (`Okhsv::to_color` is not `const`).
    fn presets() -> Self {
        let mut colors = ArrayVec::new();
        for step in 0..Self::HUES {
            let hue = step as f32 / Self::HUES as f32;
            colors.push(Okhsv::new(hue, 1.0, 1.0).to_color());
        }
        for value in Self::NEUTRALS {
            colors.push(Okhsv::new(0.0, 0.0, value).to_color());
        }
        Self { colors }
    }

    pub(crate) fn colors(&mut self) -> &[RgbaF32] {
        self.seed();
        &self.colors
    }

    fn seed(&mut self) {
        if self.colors.is_empty() {
            *self = Self::presets();
        }
    }

    /// Puts `color` at the front, dropping any earlier copy and the oldest entry once full.
    pub(crate) fn push(&mut self, color: RgbaF32) {
        self.seed();
        if self.colors.first() == Some(&color) {
            return;
        }
        self.colors.retain(|held| *held != color);
        if self.colors.len() == Self::CAP {
            self.colors.pop();
        }
        self.colors.insert(0, color);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::paint::color::RgbaF32;
    use crate::widgets::color_picker::history::History;

    impl History {
        pub(crate) fn peek(&self) -> &[RgbaF32] {
            &self.colors
        }
    }
}

#[cfg(test)]
mod tests;
