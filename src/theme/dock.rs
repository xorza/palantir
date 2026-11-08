//! What a dock paints that a tab strip does not: the drop preview and the pointer-trailing chip.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::look::WidgetLook;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;
use glam::Vec2;

/// Visuals for [`crate::DockView`], dock-specific only: dividers read [`Theme::splitter`](crate::Theme::splitter) and pane strips [`Theme::tabs`](crate::Theme::tabs), leaving the drag feedback.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct DockTheme {
    /// The region a drop would occupy: its wash, outline and corners.
    pub preview: Chrome,
    /// The chip trailing the pointer while a tab is dragged.
    pub ghost: WidgetLook,
    /// Inset between the ghost chip's edges and its label.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub ghost_padding: Spacing,
    /// Where the ghost chip sits relative to the pointer.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::offset2")]
    pub ghost_offset: Vec2,
}

impl DockTheme {
    /// Drop previews and the drag ghost, both drawn in the palette's accent. The ghost reads as a tab lifted off its strip: a tab's corners and inset, on the strip's `surround` band.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            preview: Chrome::rounded(p.accent.with_alpha8(46), m.radius.mark)
                .with_border(Stroke::new(p.accent, m.stroke.fine)),
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
