//! What one menu row wears in each of its four interaction states, picked or not.

use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::math::domain;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::theme::look::WidgetLook;
use crate::theme::look::engagement::Engagement;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::theme::look::stateful_look::StatefulLook;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;
use crate::theme::selection::Selection;
use crate::theme::text_style::TextStyle;
use crate::theme::text_style_overrides::TextStyleOverrides;
use glam::Vec2;

/// Four-state row looks for [`crate::MenuItem`] (`active` = pressed), one pack for a row that is
/// not the pick and one for the row that is, as the current choice of a combo list, and the
/// check mark's gutter. `active` defaults to `hovered`: a click closes the menu.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::ThemeWalk,
)]
#[serde(deny_unknown_fields)]
pub struct MenuItemTheme {
    /// The four per-state looks of a row that is not the pick (`[menu.item.unselected.normal]`).
    pub unselected: StatefulLook,
    /// The four per-state looks of the row that is the pick (`[menu.item.selected.normal]`).
    pub selected: StatefulLook,
    /// Text axes the shortcut hint sets over its row's text: its face, size and, on a resting
    /// live row, its ink; a hovered, pressed or disabled row's hint takes its label's ink, as
    /// [`TooltipTheme::text`](crate::TooltipTheme) and the color picker's labels set theirs.
    #[serde(default, skip_serializing_if = "TextStyleOverrides::is_empty")]
    pub shortcut: TextStyleOverrides,
    /// Minimum gutter between the label and its shortcut hint (the row is `SpaceBetween`).
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub gap: f32,
    /// Side of the square gutter every row keeps left of its label for the check mark a
    /// [checked](crate::MenuItem::checked) or [picked](crate::MenuItem::selected) row shows.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub check_size: f32,
    /// Stroke width of the check mark, which paints in the row's text ink.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub check_width: f32,
    /// The check mark's three points as fractions of [`Self::check_size`], so the tick keeps its
    /// proportions at any size.
    #[serde(deserialize_with = "crate::primitives::file::offset_points3")]
    pub check_points: [Vec2; 3],
    /// Gap between the check gutter and the label.
    #[serde(deserialize_with = "crate::primitives::file::length")]
    pub check_gap: f32,
    /// Spacing and transition spec; `margin` is `ZERO` since [`MenuTheme::gap`](crate::MenuTheme::gap) spaces rows.
    pub defaults: SlotDefaults,
}

impl MenuItemTheme {
    /// Rows transparent at rest, one surface step brighter on hover, under the metrics'
    /// `transition` as a button is. The pick reads in the accent ink in every live state, as
    /// the selection wash does, and keeps the row's fills. The check gutter is the body text's
    /// size, the em a `✓` glyph of the label's would take.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        // Hover is one surface step brighter (`element_mid`), as `ButtonTheme::subtle`; `active` keeps it
        // since a click closes the menu. The chip radius stays under the panel's so it nests in the corner.
        let hovered = WidgetLook::new(Chrome::rounded(
            p.element_mid,
            Corners::all(m.radius.control),
        ));
        let unselected = StatefulLook {
            normal: WidgetLook::default(),
            active: hovered.clone(),
            hovered,
            disabled: WidgetLook {
                chrome: Chrome::NONE,
                text: TextStyleOverrides::NONE.with_color(p.text_disabled),
            },
        };
        let picked = |look: &WidgetLook| WidgetLook {
            chrome: look.chrome.clone(),
            text: TextStyleOverrides::NONE.with_color(p.accent),
        };
        let selected = StatefulLook {
            normal: picked(&unselected.normal),
            hovered: picked(&unselected.hovered),
            active: picked(&unselected.active),
            disabled: unselected.disabled.clone(),
        };
        Self {
            unselected,
            selected,
            shortcut: TextStyleOverrides::NONE.with_color(p.text_muted),
            gap: m.space.xl,
            check_size: m.text.body,
            check_width: m.stroke.line,
            check_points: [
                Vec2::new(2.5 / 14.0, 7.5 / 14.0),
                Vec2::new(5.5 / 14.0, 10.5 / 14.0),
                Vec2::new(11.5 / 14.0, 3.5 / 14.0),
            ],
            check_gap: m.space.sm,
            defaults: SlotDefaults {
                padding: m.row_padding(),
                margin: Spacing::ZERO,
                transition: m.transition,
                press: None,
            },
        }
    }

    /// [`Self::check_size`] as the gutter takes it: a pixel at least, so a gutter too small to
    /// see still holds a tick inside it.
    pub(crate) const fn check_side(&self) -> f32 {
        domain::length_at_least(self.check_size, 1.0)
    }

    /// [`Self::check_points`] scaled to the [gutter's side](Self::check_side), in gutter-local
    /// pixels.
    pub(crate) fn check_polyline(&self) -> [Vec2; 3] {
        let side = self.check_side();
        self.check_points.map(|p| p * side)
    }

    /// The ink a resting live row's shortcut hint paints over `ambient`, the dimmest a menu
    /// paints, which a frosted panel holds its contrast for.
    pub(crate) fn shortcut_ink(&self, ambient: &TextStyle) -> RgbaF32 {
        let row = self.unselected.normal.text.apply(ambient);
        self.shortcut.apply(&row).color
    }
}

impl ThemeSlot for MenuItemTheme {
    type Pick = Selection;

    fn look(&self, response: &Response, selection: Selection) -> &WidgetLook {
        selection
            .pack(&self.unselected, &self.selected)
            .pick(response, Engagement::Pressed)
    }

    fn defaults(&self) -> SlotDefaults {
        self.defaults
    }
}
