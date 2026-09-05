//! The centred dialog and its input-blocking backdrop.

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::overlay_response::OverlayResponse;
use crate::widgets::overlay_scope::{Backdrop, OverlayScope};
use crate::widgets::theme::modal::ModalTheme;
use crate::widgets::widget::Widget;
use std::rc::Rc;

/// A centered dialog over a dimming, input-blocking backdrop, recorded
/// into [`Layer::Modal`] so it draws above everything and hit-tests
/// first. The panel hugs its content (floored at a min width) and centers
/// on the surface.
///
/// Dismissal: clicking the backdrop (anywhere outside the panel) or
/// pressing Esc sets [`OverlayResponse::dismissed`] — the host flips its
/// own open flag. A dialog's own "OK" button closes it from the inside
/// through the [`CloseHandle`] the body is handed. Clicks on the panel
/// itself are absorbed, so interacting with dialog content never closes
/// it.
#[derive(Debug)]
pub struct Modal<'a> {
    widget: Widget,
    chrome: Option<Background>,
    backdrop: Option<RgbaF32>,
    style: Option<&'a ModalTheme>,
}

impl<'a> Modal<'a> {
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::vstack().sense(Sense::ABSORB_POINTER),
            chrome: None,
            backdrop: None,
            style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `modal`. Takes an
    /// `Option` as readily as a reference: `.style(overrides.as_ref())`.
    ///
    /// Per-field [`Self::background`] / [`Self::backdrop`] still win over
    /// it.
    pub fn style(mut self, s: impl Into<Option<&'a ModalTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Backdrop scrim color, defaulting to [`crate::Theme::modal`]'s.
    /// One-axis hatch over the resolved bundle — see [`crate::Theme`].
    pub fn backdrop(mut self, c: RgbaF32) -> Self {
        self.backdrop = Some(c);
        self
    }

    pub fn show<R>(
        mut self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<R> {
        // The caller's identity names the *backdrop root*, but the widget
        // it arrived on is the panel — the root is framework-built below
        // under the id, and the panel moves onto a child of it.
        let root_id = self.widget.resolve(ui);

        // Handle: `mt.panel` is still borrowed at `scope.record`, which
        // owns `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let mt = self.style.unwrap_or(&ui_theme.modal);
        let dim = Background::fill(self.backdrop.unwrap_or(mt.backdrop));
        let panel_bg = self.chrome.as_ref().unwrap_or(&mt.panel);
        let theme_padding = mt.padding;
        let theme_min_width = mt.min_width;

        // The panel's own id is always derived — the caller's went to the
        // root — so this is `id`, not `default_id`.
        let panel = self
            .widget
            .id(root_id.with("panel"))
            .default_padding(theme_padding)
            .default_min_size(Size::new(theme_min_width, 0.0));

        // Root fills the surface, dims it, eats stray pointer events, and
        // centers the panel. The panel re-senses `Sense::ABSORB_POINTER`
        // so clicks on it never fall through to this dismiss-backdrop.
        let mut root = Widget::zstack()
            .id(root_id)
            .size((Sizing::FILL, Sizing::FILL))
            .child_align(Align::CENTER)
            .sense(Sense::ABSORB_POINTER);
        // No placement: a modal is a full-surface layer, and the layer's
        // own default is the surface origin with the whole surface
        // available.
        let scope = OverlayScope::claim(root_id, Layer::Modal, None, Backdrop::Root, &mut root);
        let handle = CloseHandle::default();
        let turn = scope.record(ui, |ui| {
            root.record(ui, Some(&dim), |ui| {
                panel.record(ui, Some(panel_bg), |ui| body(ui, &handle))
            })
        });
        let response = OverlayResponse {
            dismissed: turn.outside || turn.escape,
            close_requested: handle.requested(),
            inner: turn.inner,
        };
        scope.withdraw(ui, response.closed());

        response
    }
}

impl Modal<'_> {
    /// Paint `bg` as this widget's background.
    ///
    /// The panel chrome. Pass [`Background::NONE`] to suppress the themed
    /// panel chrome for this modal.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
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
