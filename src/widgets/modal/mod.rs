//! The centered dialog and its input-blocking backdrop.

use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::modal::ModalTheme;
use crate::theme::scrim::Scrim;
use crate::ui::Ui;
use crate::widget_core::click_outside::ClickOutside;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope, OverlaySpec};
use crate::widget_core::widget::Widget;
use std::rc::Rc;

/// A centered dialog over a dimming, input-blocking backdrop in [`Layer::Modal`].
/// The backdrop or Esc closes it as [`OverlayClose::Dismissed`](crate::OverlayClose::Dismissed)
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
/// [`closed`](OverlayResponse::closed), or after [`Self::close`]: a modal that merely stops
/// recording keeps every key and press one more frame, so the next key goes nowhere.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Modal<'a> {
    widget: Widget,
    chrome: Option<Chrome>,
    scrim: Option<Scrim>,
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

    /// What the modal lays over the window behind its panel, defaulting to
    /// [`crate::Theme::modal`]'s; a color makes a scrim that only dims.
    ///
    /// # Panics
    ///
    /// Panics unless the scrim's tint is a [color](crate::widget::domain::color) and its filter
    /// passes [`BackdropFilter`](crate::BackdropFilter)'s checks.
    #[track_caller]
    pub fn scrim(mut self, scrim: impl Into<Scrim>) -> Self {
        let scrim = scrim.into();
        scrim.validate();
        self.scrim = Some(scrim);
        self
    }

    /// What a press on the backdrop and Esc do: [`ClickOutside::Dismiss`], the default, closes it
    /// as [`OverlayClose::Dismissed`](crate::OverlayClose::Dismissed); [`ClickOutside::Block`]
    /// swallows both and stays open, for a dialog the user must answer, as a confirmation of
    /// unsaved work is. A modal always blocks the layers below it; a [`Popup`](crate::Popup) is
    /// the overlay presses pass through.
    pub const fn click_outside(mut self, click_outside: ClickOutside) -> Self {
        self.click_outside = click_outside;
        self
    }

    /// Records the backdrop and the panel with `body` inside.
    pub fn show<R>(mut self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> OverlayResponse<R> {
        let root_id = self.widget.resolve(ui);

        // `mt.panel` is still borrowed at `scope.record`, which owns `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let mt = self.style.unwrap_or(&ui_theme.modal);
        let dim = self.scrim.unwrap_or(mt.scrim).chrome();
        let panel_chrome = self.chrome.as_ref().unwrap_or(&mt.panel);
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
            backdrop: Backdrop::Root(self.click_outside),
        };
        let scope = OverlayScope::claim(ui, spec, &mut root);
        if scope.opened() {
            ui.overlays().arm_cancel(root_id);
        }
        let response = scope.record(ui, |ui| {
            root.record(ui, Some(&dim), |ui| {
                panel.record(ui, Some(panel_chrome), body)
            })
        });
        scope.finish(ui, response)
    }
}

impl Modal<'_> {
    /// Close the modal `id` from outside, as a caller does when a choice in its body ends it: the
    /// input it took from the layers below goes back at once. No-op for a modal its response
    /// already closed.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        ui.withdraw_input_scope(id);
    }

    /// Paints `chrome` as the panel chrome; [`Chrome::NONE`] suppresses the themed chrome.
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

    /// Paints `chrome` unless the caller set one, for a wrapper theming a
    /// widget after the caller's setters; an explicit [`Self::chrome`] wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        if self.chrome.is_none() {
            self.chrome = Some(chrome);
        }
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
