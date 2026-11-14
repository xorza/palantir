//! The centered dialog and its input-blocking barrier.

use crate::primitives::geometry::size::Size;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::modal::ModalTheme;
use crate::ui::Ui;
use crate::widget_core::click_outside::ClickOutside;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::overlay_scope::{Barrier, OverlayScope, OverlaySpec};
use crate::widget_core::widget::{MaterialBox, Widget};
use std::rc::Rc;

/// A centered dialog over a dimming, input-blocking barrier in [`Layer::Modal`].
/// The barrier or Esc closes it as [`OverlayClose::Dismissed`](crate::OverlayClose::Dismissed)
/// unless [`Self::click_outside`] blocks them; a [`Ui::close_overlay`] in the body closes it from inside.
///
/// On the frame it opens, focus moves to its first Tab stop, however it was opened, as
/// WAI-ARIA's dialog pattern asks (<https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/>),
/// and returns to the widget that held it when the modal closes. A popup moves focus only when
/// opened from the keyboard, as a native menu opened by a click highlights no row.
///
/// In the frame it opens, every light-dismiss overlay open before it closes as
/// [`OverlayClose::Dismissed`](crate::OverlayClose::Dismissed), but those it records inside: such
/// an overlay records collapsed, so the frame paints it gone. A menu on [`Layer::Menu`] would
/// otherwise sit above the scrim and take the first press; Windows ends a menu's mode as a dialog
/// activates. Record a modal before the overlays it may open over: one that already recorded in
/// the pass costs the frame a second pass.
///
/// A modal is open while it records. Stop recording it only on the frame its response reads
/// [`closed`](OverlayResponse::closed), or after
/// [`Ui::withdraw_input_scope`](crate::Ui::withdraw_input_scope) of its id, as a caller does when
/// a choice in its body ends it: a modal that merely stops recording keeps every key and press one
/// more frame, so the next key goes nowhere.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Modal<'a> {
    widget: Widget,
    chrome: Option<Chrome>,
    scrim: Option<Chrome>,
    click_outside: ClickOutside,
    style: Option<&'a ModalTheme>,
}

impl<'a> Modal<'a> {
    #[track_caller]
    /// A modal.
    pub fn new() -> Self {
        Self {
            widget: Widget::vstack().sense(Sense::ABSORB_POINTER),
            chrome: None,
            scrim: None,
            click_outside: ClickOutside::Dismiss,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::modal`](crate::Theme::modal); [`Self::chrome`] and [`Self::scrim`] still win.
    pub fn style(mut self, s: impl Into<Option<&'a ModalTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// The box the modal lays over the window behind its panel, defaulting to
    /// [`crate::Theme::modal`]'s ([`ModalTheme::scrim`]) over its material; a scrim set here wears
    /// no material, and a color makes one that only dims.
    ///
    /// # Panics
    ///
    /// Panics unless `scrim` passes its checks, as a widget's `chrome` does.
    #[track_caller]
    pub const fn scrim(mut self, scrim: Chrome) -> Self {
        scrim.validate();
        self.scrim = Some(scrim);
        self
    }

    /// What a press on the barrier and Esc do: [`ClickOutside::Dismiss`], the default, closes it
    /// as [`OverlayClose::Dismissed`](crate::OverlayClose::Dismissed); [`ClickOutside::Block`]
    /// swallows both and stays open, for a dialog the user must answer, as a confirmation of
    /// unsaved work is. A modal always blocks the layers below it; a [`Popup`](crate::Popup) is
    /// the overlay presses pass through.
    pub const fn click_outside(mut self, click_outside: ClickOutside) -> Self {
        self.click_outside = click_outside;
        self
    }

    /// Records the barrier and the panel with `body` inside.
    pub fn show<R>(mut self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R> {
        let root_id = self.widget.resolve(ui);

        // `mt.panel` is still borrowed at `scope.record`, which owns `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let mt = self.style.unwrap_or(&ui_theme.modal);
        let dim = MaterialBox::themed(self.scrim.as_ref(), &mt.scrim, mt.scrim_material.as_ref());
        let panel_box =
            MaterialBox::themed(self.chrome.as_ref(), &mt.panel, mt.panel_material.as_ref());
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        let panel = self
            .widget
            .id(root_id.with("panel"))
            .default_padding(theme_padding)
            .default_min_size(Size::new(theme_min_width, 0.0));

        // The root dims the surface; the panel re-senses `Sense::ABSORB_POINTER`.
        let mut root = Widget::zstack()
            .id(root_id)
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .sense(Sense::ABSORB_POINTER);
        let spec = OverlaySpec {
            layer: Layer::Modal,
            anchor: None,
            barrier: Barrier::Root(self.click_outside),
        };
        let scope = OverlayScope::claim(ui, spec, &mut root);
        if scope.opened() {
            ui.overlays().arm_cancel(root_id);
        }
        scope.record(ui, |ui| {
            root.record_box(ui, dim, |ui| panel.record_box(ui, panel_box, body))
        })
    }
}

impl Modal<'_> {
    /// Paints `chrome` as the panel chrome, which wears no material, in place of the theme's panel
    /// over its material; [`Chrome::NONE`] suppresses the themed chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        self.chrome = Some(chrome);
        self
    }
}

impl Configure for Modal<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
