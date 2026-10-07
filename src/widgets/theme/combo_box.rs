//! A combo box's geometry; colours come from the button and popup themes it is built from.

use crate::widgets::arrow::Arrow;
use crate::widgets::theme::palette::Palette;
use glam::Vec2;

/// Geometry for [`crate::ComboBox`]. Colours and chrome read `Theme::button` and `Theme::context_menu` (trigger as button, dropdown as context menu), so restyling either moves the combo. Left: the label-to-arrow gutter and the chevron.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ComboBoxTheme {
    /// Gutter between label and chevron; the trigger justifies its children apart, so this is the minimum gap.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Chevron bounding box in logical px; a polyline, so font-independent.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length2")]
    pub arrow_size: Vec2,
    /// Stroke width of the chevron polyline.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub arrow_width: f32,
}

impl ComboBoxTheme {
    /// Geometry only; colours come from [`ButtonTheme`](crate::ButtonTheme) and [`ContextMenuTheme`](crate::ContextMenuTheme), so the palette goes unread.
    pub const fn from_palette(_p: &Palette) -> Self {
        Self {
            gap: 12.0,
            arrow_size: Vec2::new(10.0, 6.0),
            arrow_width: 1.5,
        }
    }

    /// The chevron's three points (`v`) in a box of [`Self::arrow_size`], origin top-left; the middle point is the tip.
    pub(crate) fn chevron_pts(&self) -> [Vec2; 3] {
        Arrow {
            size: self.arrow_size,
        }
        .points()
    }
}

impl Default for ComboBoxTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
