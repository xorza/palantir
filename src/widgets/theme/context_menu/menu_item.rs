//! What one menu row wears in each of its four interaction states.

use crate::input::response::response_state::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use crate::widgets::theme::widget_look::theme_slot::{SlotDefaults, ThemeSlot};

/// Four-state row look for [`crate::widgets::context_menu::menu_item::MenuItem`]
/// (`active` = pressed). The default `active` look equals `hovered` —
/// a row's click auto-closes the menu, so a louder pressed state is
/// opt-in.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuItemTheme {
    /// The four per-state looks. `flatten` keeps theme files flat
    /// (`[context_menu.item.normal]`, not `[….item.looks.normal]`).
    #[serde(flatten)]
    pub looks: StatefulLook,
    /// RgbaF32 for the right-aligned shortcut hint (e.g. "⌘C"). Pulled
    /// off the row label color so the hint reads muted.
    pub shortcut: RgbaF32,
    /// Minimum gutter between the label and its right-aligned shortcut
    /// hint. The row is `SpaceBetween`, so this is the floor the two
    /// texts are held apart by while the menu hugs its widest row —
    /// it is what stops "Copy ⌘C" from reading as one word.
    pub gap: f32,
    /// Spacing and transition spec — see [`SlotDefaults`]. `margin` is
    /// `ZERO` by default: rows stack flush inside the menu's own padding
    /// and [`ContextMenuTheme::gap`](crate::ContextMenuTheme::gap) is
    /// what opens a gutter between them.
    #[serde(flatten)]
    pub defaults: SlotDefaults,
}

impl MenuItemTheme {
    /// `shortcut` is a bare `RgbaF32`, not a `TextStyle` — the hint is
    /// painted at the row label's size. Destructured so a new field
    /// fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            looks,
            shortcut: _,
            gap: _,
            defaults: _,
        } = self;
        looks.for_each_text(f);
    }

    /// Pick the visual state: `active` = pressed.
    pub fn pick(&self, state: &ResponseState) -> &WidgetLook {
        self.looks.pick(state, state.pressed())
    }

    pub fn from_palette(p: &Palette) -> Self {
        // Rows are transparent at rest; hover paints one surface-step
        // brighter (`elem_mid`) — same delta a menu-bar trigger uses
        // (`ButtonTheme::menu_button`), so the bar and the popup that
        // drops out of it feel like one continuous surface. `active`
        // (pressed) keeps the hover look: the click auto-closes the
        // menu, so a louder pressed state buys nothing by default.
        //
        // The chip radius stays under the panel's so it nests inside the
        // corner rather than out-rounding it: at panel radius 4 with 4 px
        // of container padding, the region a row can occupy has square
        // corners, and anything rounder than the panel itself reads as a
        // pill floating in a box.
        let hovered = WidgetLook {
            background: Background::rounded(p.elem_mid, Corners::all(3.0)),
            text: None,
        };
        Self {
            looks: StatefulLook {
                normal: WidgetLook::default(),
                active: hovered.clone(),
                hovered,
                disabled: WidgetLook {
                    background: Background::NONE,
                    text: Some(TextStyle::default().with_color(p.text_disabled)),
                },
            },
            shortcut: p.text_muted,
            // Reads against the container's 4 px: an 8 px row inset puts the
            // label 12 px off the panel edge to 9 px off its top, the
            // slightly-wider-than-tall gutter a column of labels wants.
            gap: 16.0,
            defaults: SlotDefaults {
                padding: Spacing::xy(8.0, 5.0),
                margin: Spacing::ZERO,
                anim: None,
            },
        }
    }
}

impl ThemeSlot for MenuItemTheme {
    type Pick = ();

    fn look(&self, response: &ResponseState, _pick: ()) -> &WidgetLook {
        self.pick(response)
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
