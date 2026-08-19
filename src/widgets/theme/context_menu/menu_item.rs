use crate::animation::anim_spec::AnimSpec;
use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;

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
    /// Color for the right-aligned shortcut hint (e.g. "⌘C"). Pulled
    /// off the row label color so the hint reads muted.
    pub shortcut: Color,
    /// Padding inside one row.
    pub padding: Spacing,
    /// Default margin around one row. `ZERO` by default: rows stack
    /// flush inside the menu's own padding and
    /// [`ContextMenuTheme::gap`](crate::ContextMenuTheme::gap) is what opens a gutter between them.
    pub margin: Spacing,
    /// Minimum gutter between the label and its right-aligned shortcut
    /// hint. The row is `SpaceBetween`, so this is the floor the two
    /// texts are held apart by while the menu hugs its widest row —
    /// it is what stops "Copy ⌘C" from reading as one word.
    pub gap: f32,
    /// Spec applied to fill/stroke/text transitions between row states.
    /// Default `None` — animation is opt-in (matches `ButtonTheme`).
    /// Round-trips through serde so theme files can configure motion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

impl MenuItemTheme {
    /// `shortcut` is a bare `Color`, not a `TextStyle` — the hint is
    /// painted at the row label's size. Destructured so a new field
    /// fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        let Self {
            looks,
            shortcut: _,
            padding: _,
            margin: _,
            gap: _,
            anim: _,
        } = self;
        looks.for_each_text(f);
    }

    /// Pick the visual state: `active` = pressed.
    pub fn pick(&self, state: &ResponseState) -> &WidgetLook {
        self.looks.pick(state, state.pressed())
    }

    /// Reround the row chip in every state that paints one. States with
    /// no background (`normal` by default — rows are transparent at
    /// rest) stay transparent.
    pub fn with_radius(mut self, radius: f32) -> Self {
        let corners = Corners::all(radius);
        for look in self.looks.each_mut() {
            look.background.corners = corners;
        }
        self
    }

    pub fn from_palette(p: &Palette) -> Self {
        // Rows are transparent at rest; hover paints one surface-step
        // brighter (`ELEM_HOVER`) — same delta a menu-bar trigger uses
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
            background: Background::rounded(p.elem_hover, Corners::all(3.0)),
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
            padding: Spacing::xy(8.0, 5.0),
            margin: Spacing::ZERO,
            gap: 16.0,
            anim: None,
        }
    }
}

palette_default!(MenuItemTheme);
