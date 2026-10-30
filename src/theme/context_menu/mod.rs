//! Menu theming: [`ContextMenuTheme`] here, its rows in [`menu_item`]. Menu rules wear a [`crate::SeparatorTheme`].

pub(crate) mod menu_item;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::ThemeText;
use crate::theme::context_menu::menu_item::MenuItemTheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::separator::SeparatorTheme;

/// A menu's width floor, so the menus of one bar open one width and a short menu does not read as
/// a pill: a short label and a three-key shortcut hint fit inside it. "Save As…" (66 px) and
/// "Ctrl+Shift+S" (81 px) in Inter at the 14 px body, the row's `xl` gutter (24), its two `md`
/// insets (24), the panel's two `xs` insets (8) and its hairline border (2) take 205 px; 208 is
/// the 4 px grid's next step.
const MIN_WIDTH: f32 = 208.0;

/// Visuals for [`crate::Popup`]-hosted context menus.
/// `panel` is the container chrome, `item` drives [`crate::MenuItem`] rows, `min_width` floors the container so short labels don't paint as a pill.
///
/// Read globally through [`crate::Theme::context_menu`] or per instance through the `style` of [`crate::ContextMenu`], [`crate::MenuItem`] and [`crate::MenuSeparator`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct ContextMenuTheme {
    /// Panel chrome behind the items; its `padding` carves the gutter to the rows.
    pub panel: Chrome,
    /// Padding inside the container, around the items.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
    /// Floor for the container width.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
    pub min_width: f32,
    /// Vertical gutter between rows. `0.0` stacks them flush, like native menus.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::length")]
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
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            panel: p.popup_panel(m),
            padding: Spacing::all(m.space.xs),
            min_width: MIN_WIDTH,
            gap: 0.0,
            item: MenuItemTheme::from_parts(p, m),
            separator: SeparatorTheme::menu_separator(p, m),
        }
    }
}

impl Default for ContextMenuTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DEFAULT, &Metrics::DEFAULT)
    }
}
