//! What a dock paints that a tab strip does not: the drop preview, the
//! insertion caret, and the chip trailing the pointer.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::widget_core::widget_look::WidgetLook;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;
use glam::Vec2;

/// Visuals for [`crate::DockView`] — and only for what is dock-specific.
///
/// The dividers read [`Theme::splitter`](crate::Theme::splitter) and
/// every pane's strip reads [`Theme::tabs`](crate::Theme::tabs), so a
/// bundle that would restate either of them does not exist. What is left
/// is the drag feedback.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DockTheme {
    /// Wash over the region a drop would occupy.
    pub preview_fill: RgbaF32,
    /// Outline around that region.
    pub preview_stroke: Stroke,
    /// Corner radius of the preview.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub preview_corner: f32,
    /// Breadth of the insertion mark drawn between two chips.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub caret_width: f32,
    /// The chip trailing the pointer while a tab is dragged.
    pub ghost: WidgetLook,
    /// Inset between the ghost chip's edges and its label.
    pub ghost_padding: Spacing,
    /// Where the ghost chip sits relative to the pointer.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub ghost_offset: Vec2,
    /// How far in from each edge the split wedges reach, as a fraction
    /// of the pane's content rect. `0.25` leaves the inner half as the
    /// join zone.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::fraction")]
    pub edge_fraction: f32,
}

impl DockTheme {
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            ghost,
            preview_fill: _,
            preview_stroke: _,
            preview_corner: _,
            caret_width: _,
            ghost_padding: _,
            ghost_offset: _,
            edge_fraction: _,
        } = self;
        ghost.for_each_text(f);
    }

    /// Drop previews and the drag ghost, both drawn in the palette's accent.
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            preview_fill: p.accent.with_alpha(0.18),
            preview_stroke: Stroke::new(p.accent, 1.5),
            preview_corner: 2.0,
            caret_width: 3.0,
            ghost: WidgetLook {
                background: Background::rounded(p.elem, Corners::all(4.0))
                    .with_border(Stroke::new(p.accent, 1.0)),
                text: TextStyleOverrides::NONE,
            },
            ghost_padding: Spacing::new(10.0, 4.0, 10.0, 4.0),
            ghost_offset: Vec2::new(14.0, 18.0),
            edge_fraction: 0.25,
        }
    }
}

impl Default for DockTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
