use crate::animation::AnimSpec;
use crate::input::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::WidgetLook;

/// Visuals for [`crate::widgets::popup::Popup`]-hosted context menus.
/// `panel` paints the surrounding container chrome (fill + stroke +
/// radius); `item` drives [`MenuItem`] rows. `min_width` is the
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
    /// Optional motion spec on `MenuItem` hover/press transitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anim: Option<AnimSpec>,
}

/// Three-state row look for [`crate::widgets::context_menu::MenuItem`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuItemTheme {
    pub normal: WidgetLook,
    pub hovered: WidgetLook,
    pub disabled: WidgetLook,
    /// Color for the right-aligned shortcut hint (e.g. "⌘C"). Pulled
    /// off the row label color so the hint reads muted.
    pub shortcut: Color,
    /// Padding inside one row.
    pub padding: Spacing,
}

impl MenuItemTheme {
    pub fn pick(&self, state: ResponseState) -> &WidgetLook {
        if state.disabled {
            &self.disabled
        } else if state.hovered {
            &self.hovered
        } else {
            &self.normal
        }
    }
}

impl Default for ContextMenuTheme {
    fn default() -> Self {
        let m = palette::TEXT_MUTED;
        let edge = m.with_alpha(0.22);
        let panel = Background {
            fill: palette::ELEM.into(),
            stroke: Stroke::solid(edge, 1.0),
            radius: Corners::all(6.0),
            shadow: Shadow::NONE,
        };
        let separator = m.with_alpha(0.18);
        Self {
            panel,
            padding: Spacing::all(4.0),
            min_width: 160.0,
            item: MenuItemTheme::default(),
            separator,
            anim: None,
        }
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        // Rows are transparent at rest; hover paints accent-tinted fill.
        let hover_bg = Background {
            fill: palette::ELEM_ACTIVE.into(),
            stroke: Stroke::ZERO,
            radius: Corners::all(4.0),
            shadow: Shadow::NONE,
        };
        Self {
            normal: WidgetLook::default(),
            hovered: WidgetLook {
                background: Some(hover_bg),
                text: None,
            },
            disabled: WidgetLook {
                background: None,
                text: Some(TextStyle::default().with_color(palette::TEXT_DISABLED)),
            },
            shortcut: palette::TEXT_MUTED,
            padding: Spacing::xy(10.0, 6.0),
        }
    }
}
