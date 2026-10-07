//! What one menu row wears in each of its four interaction states.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::stateful_look::StatefulLook;
use crate::widget_core::widget_look::theme_slot::{SlotDefaults, ThemeSlot};
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyleOverrides;

/// Four-state row look for [`crate::widgets::context_menu::menu_item::MenuItem`] (`active` = pressed).
/// `active` defaults to `hovered`: a click closes the menu.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuItemTheme {
    #[serde(flatten)]
    /// Four-state row look.
    pub looks: StatefulLook,
    /// Shortcut hint colour.
    pub shortcut: RgbaF32,
    /// Minimum gutter between the label and its shortcut hint (the row is `SpaceBetween`).
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Spacing and transition spec; `margin` is `ZERO` since [`ContextMenuTheme::gap`](crate::ContextMenuTheme::gap) spaces rows.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl MenuItemTheme {
    /// Destructured so a new field fails to compile here, see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            looks,
            shortcut: _,
            gap: _,
            defaults: _,
        } = self;
        looks.for_each_text(f);
    }

    /// Rows transparent at rest, one surface step brighter on hover.
    pub fn from_palette(p: &Palette) -> Self {
        // Hover is one surface step brighter (`element_mid`), as `ButtonTheme::menu_button`; `active` keeps it
        // since a click closes the menu. The chip radius stays under the panel's so it nests in the corner.
        let hovered = WidgetLook {
            background: Background::rounded(p.element_mid, Corners::all(3.0)),
            text: TextStyleOverrides::NONE,
        };
        Self {
            looks: StatefulLook {
                normal: WidgetLook::default(),
                active: hovered.clone(),
                hovered,
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            shortcut: p.text_muted,
            gap: 16.0,
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 5.0),
                margin: Spacing::ZERO,
                animation: None,
            },
        }
    }
}

impl ThemeSlot for MenuItemTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.looks.pick(response, response.pressed())
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
