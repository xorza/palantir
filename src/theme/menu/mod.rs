//! Menu theming: [`MenuTheme`] here, its rows in [`menu_item`]. Menu rules wear a [`crate::SeparatorTheme`].

pub(crate) mod menu_item;

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::material::Material;
use crate::theme::menu::menu_item::MenuItemTheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::separator::SeparatorTheme;

/// A menu's width floor, so the menus of one bar open one width and a short menu does not read as
/// a pill: a short label and a three-key shortcut hint fit inside it. "Save As…" (66 px) and
/// "Ctrl+Shift+S" (81 px) in Inter at the 14 px body, the row's `xl` gutter (24), its two `md`
/// insets (24), the panel's two `xs` insets (8) and its hairline border (2) take 205 px; 208 is
/// the 4 px grid's next step.
const MIN_WIDTH: f32 = 208.0;

/// Visuals for every [`crate::Menu`]: a context menu, a menu below its trigger, a combo list, a
/// menu bar's menus and a tab strip's overflow.
/// `panel` is the container chrome, `item` drives [`crate::MenuItem`] rows, `min_width` floors the container so short labels don't paint as a pill.
///
/// Read globally through [`crate::Theme::menu`] or per instance through the `style` of [`crate::Menu`], [`crate::MenuItem`] and [`crate::MenuSeparator`], and the `menu_style` of a widget that drops one.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[must_use]
pub struct MenuTheme {
    /// Panel chrome behind the items; its `padding` carves the gutter to the rows.
    pub panel: Chrome,
    /// The material the panel wears under its fill, drawn as a shape under it
    /// ([`MaterialPaint`](crate::MaterialPaint)); `None` keeps the panel opaque, which a file that
    /// leaves it out reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// Padding inside the container, around the items.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::padding")]
    pub padding: Spacing,
    /// Floor for the container width.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub min_width: f32,
    /// Vertical gutter between rows. `0.0` stacks them flush, like native menus.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub gap: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin divider between groups ([`crate::MenuSeparator`]).
    pub separator: SeparatorTheme,
}

impl MenuTheme {
    /// The popup panel in [`Metrics::float_material`], held for the rows' shortcut ink, with a
    /// [`MenuItemTheme`] and the menu [`SeparatorTheme`].
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let item = MenuItemTheme::from_parts(p, m);
        let ink = item.shortcut;
        let mut theme = Self {
            panel: p.popup_panel(m),
            material: None,
            padding: Spacing::all(m.space.xs),
            min_width: MIN_WIDTH,
            gap: 0.0,
            item,
            separator: SeparatorTheme::menu(p, m),
        };
        m.dress_float(&mut theme.panel, &mut theme.material, ink);
        theme
    }
}
