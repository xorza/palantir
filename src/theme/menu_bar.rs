//! What a menu bar's titles wear, closed and open, and which of the two applies.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::button::ButtonTheme;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::MenuBar`]: its titles, by whether their menu is open. A title rests flat
/// in the muted ink, so it reads as a menu rather than as a button, and fills and takes the full
/// ink under the pointer, from [`ButtonTheme::subtle`]; the title whose menu is open rests lit
/// as a hovered one, as macOS, Windows and GNOME light the open menu's title. The menus themselves
/// wear [`Theme::menu`](crate::Theme::menu).
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
pub struct MenuBarTheme {
    /// A title whose menu is closed, in each state.
    pub title: StatefulLook,
    /// The title whose menu is open.
    pub open: WidgetLook,
    /// The band behind the titles; the palette's `surround` by default, as a tab strip's.
    pub bar: Chrome,
    /// Inset between the band's edges and the titles; its sides match a tab strip's, so the first
    /// title and the first tab line up.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub bar_padding: Spacing,
    /// Gutter between two titles.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// A title's padding and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

/// Whether a menu bar title's menu is open, which picks [`MenuBarTheme::open`] over
/// [`MenuBarTheme::title`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuState {
    /// The menu is closed.
    Closed,
    /// The menu is open.
    Open,
}

impl From<bool> for MenuState {
    /// `true` is [`Self::Open`].
    fn from(open: bool) -> Self {
        if open { Self::Open } else { Self::Closed }
    }
}

impl MenuBarTheme {
    /// [`ButtonTheme::subtle`]'s flat titles in the muted ink at rest, the open one wearing
    /// the hovered look, an `xs` gutter apart, so two lit fills never touch, on the `surround`
    /// band.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let ButtonTheme {
            unselected: mut looks,
            selected: _,
            defaults,
        } = ButtonTheme::subtle(p, m);
        looks.normal.text = looks.normal.text.with_color(p.text_muted);
        Self {
            open: looks.hovered.clone(),
            title: looks,
            bar: Chrome::new(p.surround),
            bar_padding: Spacing::xy(m.space.sm, m.space.xs),
            gap: m.space.xs,
            defaults,
        }
    }
}

impl ThemeSlot for MenuBarTheme {
    type Pick = MenuState;

    /// The open look, or the closed pack by state (`active` = pressed).
    fn look(&self, response: &ResponseState, state: MenuState) -> &WidgetLook {
        match state {
            MenuState::Open => &self.open,
            MenuState::Closed => self.title.pick(response, Engagement::Pressed),
        }
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
