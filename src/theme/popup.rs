//! What a bare popup wears: the floating surface under a body no wrapper dresses.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::theme::menu::menu_item::MenuItemTheme;
use crate::theme::metrics::{FloatMaterial, MaterialRefusal, Metrics};
use crate::theme::palette::Palette;
use crate::theme::text_style::TextStyle;

/// Visuals for a [`crate::Popup`] or [`crate::PopupTrigger`] whose caller sets no chrome or
/// padding, unless its `style` names another; the menus wear [`crate::MenuTheme`], and the color
/// picker's popup its own `PopupTheme`. A popup is a floating
/// surface, so it paints one by default, as egui's `Frame::popup` and GTK's `popover` node do.
/// Refused: [`crate::Theme::panel_chrome`], the in-tree containers' fallback, which is unset in
/// the stock themes and so left a popup's rows over the page.
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
pub struct PopupTheme {
    /// Panel chrome behind the body.
    pub panel: Chrome,
    /// The material the panel wears under its fill, drawn as a shape under it
    /// ([`MaterialPaint`](crate::MaterialPaint)); `None` keeps the panel opaque, which a file that
    /// leaves it out reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// Padding inside the panel, applied when the caller leaves it unset.
    #[serde(deserialize_with = "crate::primitives::file::padding")]
    pub padding: Spacing,
}

impl PopupTheme {
    /// [`Palette::popup_panel`], the menus' and the picker's surface, at the picker's inset, in
    /// [`Metrics::float_material`] held for the muted ink, which a menu row's shortcut and the
    /// picker's channel names read in, as an app fills a bare popup with rows like a menu's.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        let mut theme = Self::undressed(p, m);
        if let Some(float) = &m.float_material {
            let ink = MenuItemTheme::from_parts(p, m).shortcut_ink(&TextStyle::from_parts(p, m));
            theme.try_dress(float, ink).ok();
        }
        theme
    }

    /// [`Self::from_parts`] before its material, for a surface held for another ink.
    pub(super) fn undressed(p: &Palette, m: &Metrics) -> Self {
        Self {
            panel: p.popup_panel(m),
            material: None,
            padding: Spacing::all(m.space.sm),
        }
    }

    /// Dress the panel in `float`, held for `ink`, the dimmest the body paints.
    pub(super) fn try_dress(
        &mut self,
        float: &FloatMaterial,
        ink: RgbaF32,
    ) -> Result<(), MaterialRefusal> {
        float.try_dress(&mut self.panel, &mut self.material, ink)
    }
}
