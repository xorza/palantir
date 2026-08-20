//! Menu theming: the [`ContextMenuTheme`] panel here and its rows in
//! [`menu_item`]. Menu *rules* have no bundle of their own — they wear
//! a [`crate::SeparatorTheme`] like any other divider.

pub(crate) mod menu_item;

use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::context_menu::menu_item::MenuItemTheme;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::separator::SeparatorTheme;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::stateful_look::StatefulLook;
use glam::Vec2;

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
pub struct ContextMenuTheme {
    /// Panel chrome behind the items. Container's `padding` carves the
    /// gutter between chrome and rows.
    pub panel: Background,
    /// Padding inside the container, around the column of items.
    pub padding: Spacing,
    /// Floor for the menu's container width.
    pub min_width: f32,
    /// Vertical gutter between rows. `0.0` (the default) stacks them
    /// flush, so a hovered row's chip meets its neighbour's — the look
    /// every native menu has. Raise it for a spaced, card-like list.
    pub gap: f32,
    /// Per-row visuals. See [`MenuItemTheme`].
    pub item: MenuItemTheme,
    /// Thin horizontal divider between groups (for
    /// [`crate::MenuItem::separator`]).
    pub separator: SeparatorTheme,
}

impl ContextMenuTheme {
    /// `panel` / `separator` are chrome only; the rows carry the text.
    /// Destructured so a new field fails to compile here — see
    /// [`Theme::for_each_text`](crate::Theme).
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
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

    /// Reround the panel and re-nest the row chips inside it. Both radii
    /// are plain fields (`panel.corners`, and each `item` look's
    /// `background.corners`), but they are not independent: a chip at or
    /// above the panel's radius out-rounds the corner it sits in, so
    /// setting one by hand and not the other is how a menu ends up
    /// looking like pills in a box. `chip` defaults to one px under
    /// `panel`, the relationship [`Self::from_palette`] ships.
    pub fn with_radius(mut self, panel: f32, chip: Option<f32>) -> Self {
        self.panel.corners = Corners::all(panel);
        let corners = Corners::all(chip.unwrap_or((panel - 1.0).max(0.0)));
        // Destructured so a new row state fails to compile here rather
        // than quietly keeping the radius this method was called to
        // change — same guarantee `for_each_text` keeps above.
        let StatefulLook {
            normal,
            hovered,
            active,
            disabled,
        } = &mut self.item.looks;
        for look in [normal, hovered, active, disabled] {
            look.background.corners = corners;
        }
        self
    }

    pub fn from_palette(p: &Palette) -> Self {
        // Radius sits on the small-floating-overlay step shared with
        // `TooltipTheme`, not the modal's 12 — the same corner that reads
        // as "soft" on a dialog reads as a bubble on a stack of 26 px
        // rows. The shadow is what separates the panel from what it
        // opened over: the fill is `elem`, the same surface tier as the
        // panels and cards underneath, so a hairline alone leaves the
        // menu looking glued down.
        let panel = Background::rounded(p.elem, Corners::all(4.0))
            .with_stroke(Stroke::solid(p.border_mid(), 1.0))
            .with_shadow(Shadow::drop(
                Color::linear_rgba(0.0, 0.0, 0.0, 0.5),
                Vec2::new(0.0, 3.0),
                6.0,
            ));
        Self {
            panel,
            padding: Spacing::all(4.0),
            min_width: 160.0,
            gap: 0.0,
            item: MenuItemTheme::from_palette(p),
            separator: SeparatorTheme::menu_separator(p),
        }
    }
}

palette_default!(ContextMenuTheme);
