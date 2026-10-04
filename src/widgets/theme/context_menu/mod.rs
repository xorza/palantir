//! Menu theming: the [`ContextMenuTheme`] panel here and its rows in
//! [`menu_item`]. Menu *rules* have no bundle of their own — they wear
//! a [`crate::SeparatorTheme`] like any other divider.

pub(crate) mod menu_item;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::background::Background;
use crate::widgets::theme::ThemeText;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::separator::SeparatorTheme;

/// Visuals for [`crate::Popup`]-hosted context menus.
/// `panel` paints the surrounding container chrome (fill + stroke +
/// radius); `item` drives [`crate::MenuItem`] rows. `min_width` is the
/// floor for the menu's container Sizing on the main axis so single-
/// character labels don't paint as a one-glyph-wide pill.
///
/// Every menu widget reads this bundle: globally through
/// [`crate::Theme::context_menu`], or per instance through
/// [`crate::ContextMenu::style`] / [`crate::MenuItem::style`] /
/// [`crate::MenuSeparator::style`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct ContextMenuTheme {
    /// Panel chrome behind the items. Container's `padding` carves the
    /// gutter between chrome and rows.
    pub panel: Background,
    /// Padding inside the container, around the column of items.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Floor for the menu's container width.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
    /// Vertical gutter between rows. `0.0` (the default) stacks them
    /// flush, so a hovered row's chip meets its neighbour's — the look
    /// every native menu has. Raise it for a spaced, card-like list.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::gap")]
    pub gap: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin horizontal divider between groups (for
    /// [`crate::MenuSeparator`]).
    pub separator: SeparatorTheme,
}

impl ContextMenuTheme {
    /// `panel` / `separator` are chrome only; the rows carry the text.
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
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

    /// The popup panel, holding a [`MenuItemTheme`] and the menu spelling of
    /// [`SeparatorTheme`].
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
