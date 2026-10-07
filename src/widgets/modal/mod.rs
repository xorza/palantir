//! The centred dialog and its input-blocking backdrop.

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope};
use crate::widget_core::widget::Widget;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::theme::modal::ModalTheme;
use std::rc::Rc;

/// A centred dialog over a dimming, input-blocking backdrop in [`Layer::Modal`].
/// The backdrop or Esc sets [`OverlayResponse::dismissed`]; the body's
/// [`CloseHandle`] closes it from inside.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Modal<'a> {
    widget: Widget,
    chrome: Option<Background>,
    backdrop: Option<RgbaF32>,
    style: Option<&'a ModalTheme>,
}

impl<'a> Modal<'a> {
    #[track_caller]
    /// A modal.
    pub fn new() -> Self {
        Self {
            widget: Widget::vstack().sense(Sense::ABSORB_POINTER),
            chrome: None,
            backdrop: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `modal`; [`Self::background`] and [`Self::backdrop`] still win.
    pub fn style(mut self, s: impl Into<Option<&'a ModalTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Backdrop scrim colour, defaulting to [`crate::Theme::modal`]'s.
    ///
    /// # Panics
    ///
    /// Panics unless `c` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn backdrop(mut self, c: RgbaF32) -> Self {
        self.backdrop = Some(domain::color(c));
        self
    }

    /// Records the backdrop and the panel with `body` inside, handed a [`CloseHandle`].
    pub fn show<R>(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<R> {
        let root_id = self.widget.resolve(ui);

        // `mt.panel` is still borrowed at `scope.record`, which owns `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let mt = self.style.unwrap_or(&ui_theme.modal);
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let panel_bg = self.chrome.as_ref().unwrap_or(&mt.panel);
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
        let scope = OverlayScope::claim(ui, Layer::Modal, None, Backdrop::Root, &mut root);
        let handle = CloseHandle::default();
        let turn = scope.record(ui, |ui| {
            root.record(ui, Some(&dim), |ui| {
                panel.record(ui, Some(panel_bg), |ui| body(ui, &handle))
            })
        });
        let response = OverlayResponse {
            id: root_id,
            dismissed: turn.outside || turn.escape,
            close_requested: handle.requested(),
            inner: turn.inner,
        };
        scope.withdraw(ui, response.closed());

        response
    }
}

impl Modal<'_> {
    /// Paints `background` as the panel chrome; [`Background::NONE`] suppresses the themed chrome.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        background.validate();
        self.chrome = Some(background);
        self
    }

    /// Paints `background` unless the caller set one, for a wrapper theming a
    /// widget after the caller's setters; an explicit [`Self::background`] wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        background.validate();
        if self.chrome.is_none() {
            self.chrome = Some(background);
        }
        self
    }
}

impl Configure for Modal<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
