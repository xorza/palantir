//! A combo box's geometry. Its colours come from the button and popup
//! themes it is assembled out of, which is why they are not here.

use crate::widgets::theme::palette::Palette;
use glam::Vec2;

/// Geometry for [`crate::ComboBox`]. Colours and chrome are *not* here:
/// the trigger paints as a button and the dropdown as a context menu, so
/// those read `Theme::button` and `Theme::context_menu` and restyling
/// either moves the combo with it. What's left is the shape the widget
/// would otherwise hardcode — the gutter between label and arrow, and
/// the chevron itself.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ComboBoxTheme {
    /// Gutter between the selected label and the chevron. The trigger
    /// justifies its two children apart, so this is the *minimum* gap,
    /// not the rendered one.
    pub gap: f32,
    /// Chevron bounding box in logical px. Drawn as a polyline rather
    /// than a glyph, so it stays font-independent.
    pub arrow_size: Vec2,
    /// Stroke width of the chevron polyline.
    pub arrow_stroke: f32,
}

impl ComboBoxTheme {
    pub fn from_palette(_p: &Palette) -> Self {
        Self {
            gap: 12.0,
            arrow_size: Vec2::new(10.0, 6.0),
            arrow_stroke: 1.5,
        }
    }

    /// The chevron's three points (`v`), in a box of [`Self::arrow_size`]
    /// with the origin at the top-left. The middle point is the tip.
    pub(crate) fn chevron_pts(&self) -> [Vec2; 3] {
        let Vec2 { x: w, y: h } = self.arrow_size;
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(w * 0.5, h),
            Vec2::new(w, 0.0),
        ]
    }
}

palette_default!(ComboBoxTheme);
