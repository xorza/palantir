//! The anchored floating body: the widget and the press-outside policy.

pub(crate) mod click_outside;
pub(crate) mod popup_trigger;

use crate::input::sense::Sense;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::background::Background;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope};
use crate::widget_core::widget::Widget;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::click_outside::ClickOutside;
use std::rc::Rc;

/// A side-layer container placed relative to a screen-space anchor, drawn
/// above `Main`, escaping ancestor clip. Placement uses the body's measured
/// size, flipped or shifted to fit the surface.
///
/// The layer is a field because this is the engine under every anchored
/// overlay; a context menu is a popup on [`Layer::Menu`].
///
/// Outside clicks follow [`ClickOutside`]. Under the modal pair (`Block` /
/// `Dismiss`, the default) a full-surface click-eater sits under the body and
/// the popup owns keyboard and pointer watches for every layer below it.
/// Focus is unchanged. Use [`ClickOutside::PassThrough`] to take neither.
///
/// Implements [`Configure`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Popup {
    anchor: Anchor,
    click_outside: ClickOutside,
    layer: Layer,
    widget: Widget,
    chrome: Option<Background>,
}

impl Popup {
    /// A popup placed by `anchor`, e.g. `Popup::new(Anchor::below(rect))`.
    #[track_caller]
    pub fn new(anchor: Anchor) -> Self {
        Self {
            anchor,
            click_outside: ClickOutside::Dismiss,
            layer: Layer::Popup,
            widget: Widget::vstack().sense(Sense::CLICK),
            chrome: None,
        }
    }

    /// Record into `layer` rather than [`Layer::Popup`]; a context menu uses
    /// [`Layer::Menu`] so it can open from inside a popup.
    ///
    /// # Panics
    ///
    /// At `show`, when nested inside a layer not strictly below `layer`
    /// ([`Ui::layer`]'s rule).
    pub const fn layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// What a press outside the overlay does. Default [`ClickOutside::Dismiss`].
    pub const fn click_outside(mut self, m: ClickOutside) -> Self {
        self.click_outside = m;
        self
    }

    /// Re-anchor an already-built popup, for a wrapper whose placement is only
    /// known at `show` ([`crate::ContextMenu`]).
    pub const fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Record the overlay and its `body`, which gets a [`CloseHandle`].
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<R> {
        let Self {
            anchor,
            click_outside,
            layer,
            mut widget,
            chrome,
        } = self;
        // Resolved before the layer switch so the body and eater ids are scoped
        // to the trigger's site, not the side layer's root.
        let id = widget.resolve(ui);
        let eater_id = id.with("eater");
        // Takes the pointer and the keys from lower layers together, or neither.
        let backdrop = if click_outside == ClickOutside::PassThrough {
            Backdrop::None
        } else {
            Backdrop::Eater(eater_id)
        };
        let scope = OverlayScope::claim(ui, layer, Some(anchor), backdrop, &mut widget);

        let theme = Rc::clone(ui.theme());
        widget.configure().default_clip(theme.panel_clip);
        let chrome = chrome.as_ref().or(theme.panel_background.as_ref());
        let handle = CloseHandle::default();
        let turn = scope.record(ui, |ui| widget.record(ui, chrome, |ui| body(ui, &handle)));
        let dismiss_mode = click_outside == ClickOutside::Dismiss;
        let response = OverlayResponse {
            id,
            // `Dismiss` also closes on Esc so hosts read one `closed()` signal.
            dismissed: dismiss_mode && (turn.outside || turn.escape),
            close_requested: handle.requested(),
            inner: turn.inner,
        };
        scope.withdraw(ui, response.closed());
        response
    }
}

impl Popup {
    /// Paint `background` as this widget's background; unset falls back to
    /// `panel_background`. [`Background::NONE`] suppresses the fallback.
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

    /// Paint `background` unless the caller set one; an explicit
    /// [`Self::background`] wins in either order. For wrappers that theme a held
    /// widget after the caller's setters ran.
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

impl Configure for Popup {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
