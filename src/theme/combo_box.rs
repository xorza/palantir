//! A combo box's geometry; colors come from the button and popup themes it is built from.

use crate::primitives::geometry::arrow;
use crate::primitives::geometry::size::Size;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use glam::Vec2;

/// Geometry for [`crate::ComboBox`]. Colors and chrome read `Theme::button` and `Theme::menu` (trigger as button, dropdown as context menu), so restyling either moves the combo. Left: the label-to-arrow gutter and the chevron.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct ComboBoxTheme {
    /// Gutter between label and chevron; the trigger justifies its children apart, so this is the minimum gap.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// Chevron bounding box in logical px; a polyline, so font-independent.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::size")]
    pub arrow_size: Size,
    /// Stroke width of the chevron polyline.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub arrow_width: f32,
}

impl ComboBoxTheme {
    /// Geometry only; colors come from [`ButtonTheme`](crate::ButtonTheme) and [`MenuTheme`](crate::MenuTheme), so it reads no palette.
    pub const fn from_parts(_p: &Palette, m: &Metrics) -> Self {
        Self {
            gap: m.space.md,
            arrow_size: Size::new(10.0, 6.0),
            arrow_width: m.stroke.fine,
        }
    }

    /// The chevron's three points (`v`) in a box of [`Self::arrow_size`], origin top-left; the middle point is the tip.
    pub(crate) const fn chevron_points(&self) -> [Vec2; 3] {
        arrow::points(self.arrow_size)
    }
}
