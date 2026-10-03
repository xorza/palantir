//! The swatch row a picker keeps for itself, and the colours it starts with.

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::okhsv::Okhsv;
use tinyvec::ArrayVec;

/// Recently committed colours, most recent first, seeded with a preset row.
///
/// Seeding is what makes "empty history shows presets" a stable row rather
/// than a row that changes length on the first pick: the newest colour goes
/// to the front and the oldest preset falls off the end.
///
/// Seeded on first use, not at construction: a picker's state is taken
/// out and put back every frame, and a default that built the presets
/// spent sixteen colour conversions a frame on a placeholder overwritten
/// at once. A seeded row is never empty again, so empty means unseeded.
#[derive(Debug, Default)]
pub(crate) struct History {
    colors: ArrayVec<[RgbaF32; History::CAP]>,
}

impl History {
    /// Swatches the row holds. Twelve hues and four neutrals.
    const CAP: usize = 16;

    /// Evenly spaced hues in the row. Even spacing in Okhsv hue is even
    /// spacing to the eye, which is the whole argument for the model — so the
    /// presets are derived from it rather than hand-picked.
    const HUES: usize = 12;

    /// The neutrals filling the rest of the row: black, two greys, white.
    const NEUTRALS: [f32; 4] = [0.0, 0.35, 0.7, 1.0];

    /// The preset row, computed once when a picker first opens its history.
    /// `Okhsv::to_color` is not `const`, so this is built rather than baked;
    /// sixteen conversions is not a cost worth a lazy static.
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

    /// The row, most recent first.
    pub(crate) fn colors(&mut self) -> &[RgbaF32] {
        self.seed();
        &self.colors
    }

    fn seed(&mut self) {
        if self.colors.is_empty() {
            *self = Self::presets();
        }
    }

    /// Put `color` at the front, dropping any earlier copy of it and the
    /// oldest entry once the row is full.
    ///
    /// De-duplicating is what keeps a drag from filling the row with sixteen
    /// shades of one colour: a gesture commits once, but a session picking
    /// the same swatch twice should not lose fourteen others to it.
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

/// Reach-in for the picker's tests, which read the row a picker kept
/// without seeding one where it has not.
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
mod tests {
    use crate::primitives::paint::color::RgbaF32;
    use crate::widgets::color_picker::history::History;

    /// The row starts full, so it never changes length as colours arrive —
    /// though a default history holds nothing until the row is first read.
    #[test]
    fn presets_fill_the_row() {
        let mut history = History::default();
        assert!(history.colors.is_empty(), "no presets built before use");
        assert_eq!(history.colors().len(), History::CAP);
    }

    /// The presets are the derived list, not a hand-typed one: the last hue
    /// is eleven twelfths round the circle, and the four neutrals close the
    /// row at black and white.
    #[test]
    fn presets_are_the_derived_list() {
        use crate::primitives::paint::color::okhsv::Okhsv;
        let mut history = History::default();
        let colors = history.colors();
        assert_eq!(colors[11], Okhsv::new(11.0 / 12.0, 1.0, 1.0).to_color());
        assert_eq!(colors[12], RgbaF32::BLACK);
        assert_eq!(colors[15].to_srgba_u8(), RgbaF32::WHITE.to_srgba_u8());
    }

    #[test]
    fn a_pick_moves_to_the_front_and_the_row_keeps_its_length() {
        let mut history = History::default();
        let last = history.colors()[History::CAP - 1];
        let picked = RgbaF32::hex(0x4cd3ff);
        history.push(picked);
        assert_eq!(history.colors()[0], picked);
        assert_eq!(history.colors().len(), History::CAP);
        assert!(!history.colors().contains(&last), "the oldest fell off");
    }

    /// Picking a colour already in the row moves it rather than adding it, so
    /// the row cannot fill with one colour.
    #[test]
    fn a_repeat_pick_moves_instead_of_growing() {
        let mut history = History::default();
        let held = history.colors()[5];
        history.push(RgbaF32::hex(0x123456));
        history.push(held);
        assert_eq!(history.colors()[0], held);
        assert_eq!(history.colors().len(), History::CAP);
        assert_eq!(
            history.colors().iter().filter(|c| **c == held).count(),
            1,
            "one copy only",
        );
    }
}
