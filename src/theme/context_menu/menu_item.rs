//! What one menu row wears in each of its four interaction states.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::ThemeText;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Four-state row look for [`crate::widgets::context_menu::menu_item::MenuItem`] (`active` = pressed).
/// `active` defaults to `hovered`: a click closes the menu.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MenuItemTheme {
    /// Four-state row look.
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// Shortcut hint color on a resting live row; a hovered, pressed or disabled row's hint takes
    /// its label's ink.
    pub shortcut: RgbaF32,
    /// Minimum gutter between the label and its shortcut hint (the row is `SpaceBetween`).
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
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

    /// Rows transparent at rest, one surface step brighter on hover, under the metrics'
    /// `transition` as a button is.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        // Hover is one surface step brighter (`element_mid`), as `ButtonTheme::menu_button`; `active` keeps it
        // since a click closes the menu. The chip radius stays under the panel's so it nests in the corner.
        let hovered = WidgetLook {
            chrome: Chrome::rounded(p.element_mid, Corners::all(m.radius.control)),
            text: TextStyleOverrides::NONE,
        };
        Self {
            looks: StatefulLook {
                normal: WidgetLook::default(),
                active: hovered.clone(),
                hovered,
                disabled: WidgetLook {
                    chrome: Chrome::NONE,
                    text: TextStyleOverrides::NONE.with_color(p.text_disabled),
                },
            },
            shortcut: p.text_muted,
            gap: m.space.xl,
            defaults: SlotDefaults {
                padding: m.row_padding(),
                margin: Spacing::ZERO,
                animation: m.transition,
            },
        }
    }
}

impl ThemeSlot for MenuItemTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.looks.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
