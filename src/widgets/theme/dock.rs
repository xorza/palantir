//! What a dock paints that a tab strip does not: the drop preview, the
//! insertion caret, and the chip trailing the pointer.

use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
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
    pub preview_corner: f32,
    /// Breadth of the insertion mark drawn between two chips.
    pub caret_width: f32,
    /// The chip trailing the pointer while a tab is dragged.
    pub ghost: WidgetLook,
    /// Inset between the ghost chip's edges and its label.
    pub ghost_padding: Spacing,
    /// Where the ghost chip sits relative to the pointer.
    pub ghost_offset: Vec2,
    /// How far in from each edge the split wedges reach, as a fraction
    /// of the pane's content rect. `0.25` leaves the inner half as the
    /// join zone.
    pub edge_fraction: f32,
}

impl DockTheme {
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
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

    pub fn from_palette(p: &Palette) -> Self {
        Self {
            preview_fill: p.accent.with_alpha(0.18),
            preview_stroke: Stroke::solid(p.accent, 1.5),
            preview_corner: 2.0,
            caret_width: 3.0,
            ghost: WidgetLook {
                background: Background::rounded(p.elem, Corners::all(4.0))
                    .with_stroke(Stroke::solid(p.accent, 1.0)),
                text: None,
            },
            ghost_padding: Spacing::new(10.0, 4.0, 10.0, 4.0),
            ghost_offset: Vec2::new(14.0, 18.0),
            edge_fraction: 0.25,
        }
    }
}

palette_default!(DockTheme);
