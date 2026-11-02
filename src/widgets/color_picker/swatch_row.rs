//! Where a color picker's swatch row takes its colors from.

use crate::primitives::paint::color::RgbaF32;

/// The swatch row of a [`ColorPicker`](crate::ColorPicker): one choice, so no two setters can
/// conflict. A click on a swatch picks its color.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SwatchRow<'a> {
    /// No row.
    #[default]
    None,
    /// A row the picker keeps itself: presets, with each committed pick moved to the front.
    History,
    /// A row the app owns; the picker reads it and never writes it.
    Fixed(&'a [RgbaF32]),
}
