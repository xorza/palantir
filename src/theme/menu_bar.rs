//! What a menu bar's titles wear, shut and open, and which of the two applies.

use crate::input::interaction::response_state::ResponseState;
use crate::theme::ThemeText;
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
/// ink under the pointer, from [`ButtonTheme::menu_button`]; the title whose menu is open rests lit
/// as a hovered one, as macOS, Windows and GNOME light the open menu's title. The menus themselves
/// wear [`Theme::context_menu`](crate::Theme::context_menu).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MenuBarTheme {
    /// A title whose menu is shut, in each state.
    pub title: StatefulLook,
    /// The title whose menu is open.
    pub open: WidgetLook,
    /// Gutter between two titles.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub gap: f32,
    /// A title's padding and transition spec — see [`SlotDefaults`].
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

/// Whether a menu bar title's menu is open, which picks [`MenuBarTheme::open`] over
/// [`MenuBarTheme::title`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuState {
    /// The menu is shut.
    Shut,
    /// The menu is open.
    Open,
}

impl MenuBarTheme {
    /// [`ButtonTheme::menu_button`]'s flat titles in the muted ink at rest, the open one wearing
    /// the hovered look, an `xs` gutter apart, so two lit fills never touch.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let ButtonTheme {
            mut looks,
            defaults,
        } = ButtonTheme::menu_button(p, m);
        looks.normal.text = looks.normal.text.with_color(p.text_muted);
        Self {
            open: looks.hovered.clone(),
            title: looks,
            gap: m.space.xs,
            defaults,
        }
    }

    /// Visit every text slot this theme owns; destructured so a new field fails to compile here.
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            title,
            open,
            gap: _,
            defaults: _,
        } = self;
        title.for_each_text(f);
        open.for_each_text(f);
    }
}

impl ThemeSlot for MenuBarTheme {
    type Pick = MenuState;

    /// The open look, or the shut pack by state (`active` = pressed).
    fn look(&self, response: &ResponseState, state: MenuState) -> &WidgetLook {
        match state {
            MenuState::Open => &self.open,
            MenuState::Shut => self.title.pick(response, Engagement::Pressed),
        }
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for MenuBarTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DEFAULT, &Metrics::DEFAULT)
    }
}
