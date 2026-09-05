//! The centred dialog and its input-blocking backdrop.

use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::placement::Placement;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::overlay_response::OverlayResponse;
use crate::widgets::overlay_scope::{Backdrop, OverlayScope};
use crate::widgets::theme::modal::ModalTheme;
use crate::widgets::widget::Widget;
use glam::Vec2;
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

    style_setter!(
        'a,
        ModalTheme,
        modal,
        "Per-field [`Self::background`] / [`Self::backdrop`] still win over it.",
    );

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
        let surface = ui.display().logical_rect();
        // The caller's identity names the *backdrop root*, but the widget
        // it arrived on is the panel — the root is framework-built below
        // under the id, and the panel moves onto a child of it.
        let root_id = self.widget.resolve(ui);

        // Handle: `mt.panel` is still borrowed at `scope.record`, which
        // owns `ui` mutably.
        let ui_theme = Rc::clone(ui.theme());
        let mt = self.slot(&ui_theme);
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
        let placement = Placement::fixed(Vec2::ZERO, Some(surface.size));
        let scope =
            OverlayScope::claim(root_id, Layer::Modal, placement, Backdrop::Root, &mut root);
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

impl_background!(
    Modal<'_>,
    "The panel chrome. Pass [`Background::NONE`] to suppress the themed panel \
     chrome for this modal.",
);
impl_configure!(Modal<'_>);

#[cfg(test)]
mod tests;
