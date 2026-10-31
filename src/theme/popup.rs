//! What a bare popup wears: the floating surface under a body no wrapper dresses.

use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for a [`crate::Popup`] or [`crate::PopupTrigger`] whose caller sets no chrome or
/// padding; the menus, the combo list and the picker wear their own slots. A popup is a floating
/// surface, so it paints one by default, as egui's `Frame::popup` and GTK's `popover` node do.
/// Refused: [`crate::Theme::panel_chrome`], the in-tree containers' fallback, which is unset in
/// the stock themes and so left a popup's rows over the page.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PopupTheme {
    /// Panel chrome behind the body.
    pub panel: Chrome,
    /// Padding inside the panel, applied when the caller leaves it unset.
    #[serde(deserialize_with = "crate::primitives::packed::serde::checked::padding")]
    pub padding: Spacing,
}

impl PopupTheme {
    /// [`Palette::popup_panel`], the menus' and the picker's surface, at the picker's inset.
    pub fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            panel: p.popup_panel(m),
            padding: Spacing::all(m.space.sm),
        }
    }
}

impl Default for PopupTheme {
    fn default() -> Self {
        Self::from_parts(&Palette::DARK, &Metrics::DEFAULT)
    }
}
