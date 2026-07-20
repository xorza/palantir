use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::{StatefulLook, WidgetLook};

/// Visuals for [`crate::Popup`]-hosted context menus.
/// `panel` paints the surrounding container chrome (fill + stroke +
/// radius); `item` drives [`crate::MenuItem`] rows. `min_width` is the
/// floor for the menu's container Sizing on the main axis so single-
/// character labels don't paint as a one-glyph-wide pill.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContextMenuTheme {
    /// Panel chrome behind the items. Container's `padding` carves the
    /// gutter between chrome and rows.
    pub panel: Background,
    /// Padding inside the container, around the column of items.
    pub padding: Spacing,
    /// Floor for the menu's container width.
    pub min_width: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin horizontal divider between groups (for `MenuItem::separator`).
    pub separator: Color,
}

impl ContextMenuTheme {
    /// `panel` / `separator` are chrome only; the rows carry the text.
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.item.for_each_text(f);
    }
}

/// Four-state row look for [`crate::widgets::context_menu::MenuItem`]
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
}

impl MenuItemTheme {
    pub(crate) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.looks.for_each_text(f);
    }

    /// Pick the visual state: `active` = pressed.
    pub fn pick(&self, state: &ResponseState) -> &WidgetLook {
        self.looks.pick(state, state.pressed())
    }
}

impl ContextMenuTheme {
    pub fn from_palette(p: &Palette) -> Self {
        let panel = Background::rounded(p.elem, Corners::all(6.0))
            .with_stroke(Stroke::solid(p.border_mid(), 1.0));
        Self {
            panel,
            padding: Spacing::all(4.0),
            min_width: 160.0,
            item: MenuItemTheme::from_palette(p),
            separator: p.border_soft(),
        }
    }
}

impl Default for ContextMenuTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

impl MenuItemTheme {
    pub fn from_palette(p: &Palette) -> Self {
        // Rows are transparent at rest; hover paints one surface-step
        // brighter (`ELEM_HOVER`) — same delta a menu-bar trigger uses
        // (`ButtonTheme::menu_button`), so the bar and the popup that
        // drops out of it feel like one continuous surface. `active`
        // (pressed) keeps the hover look: the click auto-closes the
        // menu, so a louder pressed state buys nothing by default.
        let hovered = WidgetLook {
            background: Some(Background::rounded(p.elem_hover, Corners::all(4.0))),
            text: None,
        };
        Self {
            looks: StatefulLook {
                normal: WidgetLook::default(),
                active: hovered.clone(),
                hovered,
                disabled: WidgetLook {
                    background: None,
                    text: Some(TextStyle::default().with_color(p.text_disabled)),
                },
            },
            shortcut: p.text_muted,
            padding: Spacing::xy(10.0, 6.0),
        }
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
