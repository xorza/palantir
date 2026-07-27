use crate::input::response::ResponseState;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::widgets::theme::palette::Palette;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::theme::widget_look::{StatefulLook, WidgetLook};
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
    pub separator: MenuSeparatorTheme,
}

impl ContextMenuTheme {
    /// `panel` / `separator` are chrome only; the rows carry the text.
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.item.for_each_text(f);
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
        self.item = self
            .item
            .with_radius(chip.unwrap_or((panel - 1.0).max(0.0)));
        self
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
    /// Minimum gutter between the label and its right-aligned shortcut
    /// hint. The row is `SpaceBetween`, so this is the floor the two
    /// texts are held apart by while the menu hugs its widest row —
    /// it is what stops "Copy ⌘C" from reading as one word.
    pub gap: f32,
}

impl MenuItemTheme {
    pub(super) fn for_each_text<F: FnMut(&mut TextStyle)>(&mut self, f: &mut F) {
        self.looks.for_each_text(f);
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
            if let Some(bg) = &mut look.background {
                bg.corners = corners;
            }
        }
        self
    }
}

impl ContextMenuTheme {
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
            separator: MenuSeparatorTheme::from_palette(p),
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
        //
        // The chip radius stays under the panel's so it nests inside the
        // corner rather than out-rounding it: at panel radius 4 with 4 px
        // of container padding, the region a row can occupy has square
        // corners, and anything rounder than the panel itself reads as a
        // pill floating in a box.
        let hovered = WidgetLook {
            background: Some(Background::rounded(p.elem_hover, Corners::all(3.0))),
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
            // Reads against the container's 4 px: an 8 px row inset puts the
            // label 12 px off the panel edge to 9 px off its top, the
            // slightly-wider-than-tall gutter a column of labels wants.
            padding: Spacing::xy(8.0, 5.0),
            gap: 16.0,
        }
    }
}

impl Default for MenuItemTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}

/// Visuals for [`crate::MenuSeparator`], the divider between menu
/// groups. Its own bundle rather than a reach into
/// [`crate::Theme::separator`]: a menu rule is a different object from
/// an in-flow one — it spans a 4 px-padded popup, not a content column,
/// and restyling the menu shouldn't have to drag every other rule in
/// the app along with it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MenuSeparatorTheme {
    /// Rule color.
    pub color: Color,
    /// Rule breadth in logical px.
    pub thickness: f32,
    /// Breathing room around the rule. Vertical only by default — the
    /// rule spans the full padded width, so a horizontal inset would
    /// leave it visibly short of the labels it divides.
    pub margin: Spacing,
}

impl MenuSeparatorTheme {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            color: p.border_soft(),
            thickness: 1.0,
            margin: Spacing::xy(0.0, 4.0),
        }
    }
}

impl Default for MenuSeparatorTheme {
    fn default() -> Self {
        Self::from_palette(&Palette::DEFAULT)
    }
}
