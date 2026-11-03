//! What a bare popup wears: the floating surface under a body no wrapper dresses.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

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
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
pub struct PopupTheme {
    /// Panel chrome behind the body.
    pub panel: Chrome,
    /// Padding inside the panel, applied when the caller leaves it unset.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
}

impl PopupTheme {
    /// [`Palette::popup_panel`], the menus' and the picker's surface, at the picker's inset, in
    /// [`Metrics::float_material`] held for the muted ink, which a menu row's shortcut and the
    /// picker's channel names read in, as an app fills a bare popup with rows like a menu's.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            panel: m.dress_float(p.popup_panel(m), p.text_muted),
            padding: Spacing::all(m.space.sm),
        }
    }

    /// The built-in look of `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }
}
