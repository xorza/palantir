//! Menu theming: [`ContextMenuTheme`] here, its rows in [`menu_item`]. Menu rules wear a [`crate::SeparatorTheme`].

pub(crate) mod menu_item;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::separator::SeparatorTheme;

/// Visuals for [`crate::Popup`]-hosted context menus.
/// `panel` is the container chrome, `item` drives [`crate::MenuItem`] rows, `min_width` floors the container so short labels don't paint as a pill.
///
/// Read globally through [`crate::Theme::context_menu`] or per instance through the `style` of [`crate::ContextMenu`], [`crate::MenuItem`] and [`crate::MenuSeparator`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct ContextMenuTheme {
    /// Panel chrome behind the items; its `padding` carves the gutter to the rows.
    pub panel: Background,
    /// Padding inside the container, around the items.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Floor for the container width.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
    /// Vertical gutter between rows. `0.0` stacks them flush, like native menus.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin divider between groups ([`crate::MenuSeparator`]).
    pub separator: SeparatorTheme,
}

impl ContextMenuTheme {
    /// Panel and separator are chrome only; rows carry the text. Destructured so a new field fails to compile; see [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(ThemeText<'_>)>(&mut self, f: &mut F) {
        let Self {
            item,
            panel: _,
            padding: _,
            min_width: _,
            gap: _,
            separator: _,
        } = self;
        item.for_each_text(f);
    }

    /// The popup panel, with a [`MenuItemTheme`] and the menu [`SeparatorTheme`].
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            panel: p.popup_panel(),
            padding: Spacing::all(4.0),
            min_width: 160.0,
            gap: 0.0,
            item: MenuItemTheme::from_palette(p),
            separator: SeparatorTheme::menu_separator(p),
        }
    }
}

impl Default for ContextMenuTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
