//! What one menu row wears in each of its four interaction states, picked or not.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::selection::Selection;
use crate::theme::text_style_overrides::TextStyleOverrides;

/// Four-state row looks for [`crate::MenuItem`] (`active` = pressed), one pack for a row that is
/// not the pick and one for the row that is, as the current choice of a combo list. `active`
/// defaults to `hovered`: a click closes the menu.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct MenuItemTheme {
    /// The four per-state looks of a row that is not the pick; `flatten` keeps theme files flat
    /// (`[menu.item.normal]`).
    #[serde(flatten)]
    pub unselected: StatefulLook,
    /// The four per-state looks of the row that is the pick (`[menu.item.selected.normal]`).
    pub selected: StatefulLook,
    /// Shortcut hint color on a resting live row; a hovered, pressed or disabled row's hint takes
    /// its label's ink.
    pub shortcut: RgbaF32,
    /// Minimum gutter between the label and its shortcut hint (the row is `SpaceBetween`).
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// Spacing and transition spec; `margin` is `ZERO` since [`MenuTheme::gap`](crate::MenuTheme::gap) spaces rows.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl MenuItemTheme {
    /// Rows transparent at rest, one surface step brighter on hover, under the metrics'
    /// `transition` as a button is. The pick reads in the accent ink in every live state, as
    /// the selection wash does, and keeps the row's fills.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        // Hover is one surface step brighter (`element_mid`), as `ButtonTheme::subtle`; `active` keeps it
        // since a click closes the menu. The chip radius stays under the panel's so it nests in the corner.
        let hovered = WidgetLook {
            chrome: Chrome::rounded(p.element_mid, Corners::all(m.radius.control)),
            text: TextStyleOverrides::NONE,
        };
        let unselected = StatefulLook {
            normal: WidgetLook::default(),
            active: hovered.clone(),
            hovered,
            disabled: WidgetLook {
                chrome: Chrome::NONE,
                text: TextStyleOverrides::NONE.with_color(p.text_disabled),
            },
        };
        let picked = |look: &WidgetLook| WidgetLook {
            chrome: look.chrome.clone(),
            text: TextStyleOverrides::NONE.with_color(p.accent),
        };
        let selected = StatefulLook {
            normal: picked(&unselected.normal),
            hovered: picked(&unselected.hovered),
            active: picked(&unselected.active),
            disabled: unselected.disabled.clone(),
        };
        Self {
            unselected,
            selected,
            shortcut: p.text_muted,
            gap: m.space.xl,
            defaults: SlotDefaults {
                padding: m.row_padding(),
                margin: Spacing::ZERO,
                transition: m.transition,
            },
        }
    }
}

impl ThemeSlot for MenuItemTheme {
    type Pick = Selection;

    fn look(&self, response: &ResponseState, selection: Selection) -> &WidgetLook {
        let pack = match selection {
            Selection::Unselected => &self.unselected,
            Selection::Selected => &self.selected,
        };
        pack.pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
