//! What a dock paints that a tab strip does not: the drop preview and the pointer-trailing chip.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use glam::Vec2;

/// Visuals for [`crate::DockView`], dock-specific only: dividers read [`Theme::splitter`](crate::Theme::splitter) and pane strips [`Theme::tabs`](crate::Theme::tabs), leaving the drag feedback.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DockTheme {
    /// Wash over the region a drop would occupy.
    pub preview_fill: RgbaF32,
    /// Outline around that region.
    pub preview_stroke: Stroke,
    /// Corner radius of the preview.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub preview_radius: f32,
    /// The chip trailing the pointer while a tab is dragged.
    pub ghost: WidgetLook,
    /// Inset between the ghost chip's edges and its label.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub ghost_padding: Spacing,
    /// Where the ghost chip sits relative to the pointer.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::offset2")]
    pub ghost_offset: Vec2,
}

impl DockTheme {
    /// Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            ghost,
            preview_fill: _,
            preview_stroke: _,
            preview_radius: _,
            ghost_padding: _,
            ghost_offset: _,
        } = self;
        ghost.for_each_text(f);
    }

    /// Drop previews and the drag ghost, both drawn in the palette's accent. The ghost reads as a tab lifted off its strip: a tab's corners and inset, on the strip's `surround` band.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            preview_fill: p.accent.with_alpha8(46),
            preview_stroke: Stroke::new(p.accent, m.stroke.fine),
            preview_radius: m.radius.mark,
            ghost: WidgetLook {
                chrome: Chrome::rounded(p.surround, Corners::all(m.radius.card))
                    .with_border(Stroke::new(p.accent, m.stroke.hairline)),
                text: TextStyleOverrides::NONE,
            },
            ghost_padding: m.row_padding(),
            ghost_offset: Vec2::new(m.space.md, m.space.lg),
        }
    }
}

impl Default for DockTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
