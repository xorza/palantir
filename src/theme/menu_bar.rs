//! What a menu bar's band and titles wear.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::button::ButtonTheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::MenuBar`]: its band, and its titles, a button each, picked by whether
/// its menu is open, as [`Selection::Selected`](crate::widget::Selection::Selected). A title rests
/// flat in the muted ink, so it reads as a menu rather than as a button, and fills and takes the
/// full ink under the pointer, from [`ButtonTheme::subtle`]; the title whose menu is open rests
/// lit as a hovered one, as macOS, Windows and GNOME light the open menu's title. The menus
/// themselves wear [`Theme::menu`](crate::Theme::menu). Refused: a pick enum and a slot of its own,
/// which re-declared a button with a selected pack.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct MenuBarTheme {
    /// A title, its `unselected` pack while its menu is closed and its `selected` pack while it is
    /// open, with its padding and transition.
    pub title: ButtonTheme,
    /// The band behind the titles; the palette's `surround` by default, as a tab strip's.
    pub bar: Chrome,
    /// Inset between the band's edges and the titles; its sides match a tab strip's, so the first
    /// title and the first tab line up.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub bar_padding: Spacing,
    /// Gutter between two titles.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
}

impl MenuBarTheme {
    /// [`ButtonTheme::subtle`]'s flat titles in the muted ink at rest, the open one wearing the
    /// hovered look in every state, an `xs` gutter apart, so two lit fills never touch, on the
    /// `surround` band.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let mut title = ButtonTheme::subtle(p, m);
        title.unselected.normal.text = title.unselected.normal.text.with_color(p.text_muted);
        let open = title.unselected.hovered.clone();
        for look in title.selected.each_mut() {
            look.clone_from(&open);
        }
        Self {
            title,
            bar: Chrome::new(p.surround),
            bar_padding: Spacing::xy(m.space.sm, m.space.xs),
            gap: m.space.xs,
        }
    }
}
