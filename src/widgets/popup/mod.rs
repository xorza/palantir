//! The anchored floating body: the widget and the press-outside policy.

pub(crate) mod click_outside;
pub(crate) mod popup_trigger;
pub(crate) mod trigger_state;

use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::axis::Axis;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::menu::MenuTheme;
use crate::theme::popup::PopupTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::overlay_scope::{Backdrop, OverlayScope, OverlaySpec};
use crate::widget_core::widget::Widget;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::click_outside::ClickOutside;
use glam::Vec2;
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
/// the popup owns keyboard and pointer watches for everything below it.
/// Focus is unchanged. Use [`ClickOutside::PassThrough`] to take neither.
///
/// A popup the caller shows is open while it records. Stop recording it only on the frame its
/// response reads [`closed`](OverlayResponse::closed), or after [`Self::close`]: a popup that
/// merely stops recording keeps the keys and the pointer of the layers below one more frame.
///
/// Implements [`Configure`].
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Popup<'a> {
    anchor: Option<Anchor>,
    click_outside: ClickOutside,
    layer: Layer,
    widget: Widget,
    chrome: Option<Chrome>,
    style: Option<&'a PopupTheme>,
}

impl<'a> Popup<'a> {
    /// A popup at the surface origin, with the whole surface to fit in; [`Self::anchor`] places
    /// it, e.g. `Popup::new().anchor(Anchor::below(rect))`.
    #[track_caller]
    pub fn new() -> Self {
        Self {
            anchor: None,
            click_outside: ClickOutside::Dismiss,
            layer: Layer::Popup,
            widget: Widget::vstack().sense(Sense::CLICK),
            chrome: None,
            style: None,
        }
    }

    /// Per-instance override of [`Theme::popup`](crate::Theme::popup): the panel and padding a
    /// caller leaves unset. [`Self::chrome`] still wins.
    pub fn style(mut self, s: impl Into<Option<&'a PopupTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Record into `layer` rather than [`Layer::Popup`]; a context menu uses
    /// [`Layer::Menu`]. Opened inside another overlay, the popup paints above it
    /// either way ([`Ui::layer`]).
    pub const fn layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// What a press outside the overlay does. Default [`ClickOutside::Dismiss`].
    pub const fn click_outside(mut self, click_outside: ClickOutside) -> Self {
        self.click_outside = click_outside;
        self
    }

    /// Where the popup sits; the surface origin when unset.
    pub const fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Close the popup `id` from outside, as a caller does when it stops showing it for a reason
    /// of its own: the input the popup took from the layers below goes back at once. No-op for a
    /// popup that holds none ([`ClickOutside::PassThrough`]), or one its response already closed.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        ui.release_input_scope(id);
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
            style,
        } = self;
        // Resolved before the layer switch so the body and eater ids are scoped
        // to the trigger's site, not the side layer's root.
        let id = widget.resolve(ui);
        let margin = widget.authored_margin().unwrap_or(Spacing::ZERO);
        let anchor = anchor.map(|anchor| Self::landed(ui, id, anchor, margin));
        let eater_id = Self::eater_id(id);
        // Takes the pointer and the keys from lower layers together, or neither.
        let backdrop = if click_outside == ClickOutside::PassThrough {
            Backdrop::None
        } else {
            Backdrop::Eater(eater_id)
        };
        let spec = OverlaySpec {
            layer,
            anchor,
            backdrop,
        };
        let scope = OverlayScope::claim(ui, spec, &mut widget);
        // A body with no rect from the last frame did not record then.
        let opened = ui.response_for(id).rect.is_none();

        let theme = Rc::clone(ui.theme());
        widget.configure().default_clip(theme.panel_clip);
        let slot = style.unwrap_or(&theme.popup);
        widget.configure().default_padding(slot.padding);
        let chrome = chrome.as_ref().unwrap_or(&slot.panel);
        let handle = CloseHandle::default();
        let turn = scope.record(ui, |ui| {
            widget.record(ui, Some(chrome), |ui| body(ui, &handle))
        });
        let dismiss_mode = click_outside == ClickOutside::Dismiss;
        let response = OverlayResponse {
            id,
            opened,
            // `Dismiss` also closes on Esc so hosts read one `closed()` signal.
            dismissed: dismiss_mode && (turn.pressed_outside || turn.escape_pressed),
            close_requested: handle.requested() || turn.close_requested,
            inner: turn.inner,
        };
        scope.finish(ui, response)
    }
}

/// Where an open popup asked to sit, and where its first frame landed it.
#[derive(Clone, Copy, Debug)]
struct Landing {
    asked: Anchor,
    landed: Option<Vec2>,
}

impl Popup<'_> {
    /// The anchor the popup `id` records at. A popup asked to sit at a point, as a context menu
    /// raised at the pointer is, sits at `asked` on its first frame there, then at the point that
    /// frame landed it. The window shifts an anchored body to fit each frame it is placed, so one
    /// whose body grows or shrinks would move under the pointer, as a list a query filters does;
    /// held where it landed, it moves again only when it stops fitting there, or when it is asked
    /// to sit somewhere else. Win32's `TrackPopupMenu` places a menu at a point once
    /// (<https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu>). A
    /// popup asked to sit beside a rect, a list under its button, is placed each frame, as it
    /// keeps the rect clear: a body that grows past the space on one side flips to the other.
    /// The anchor places the body's `margin` box, so the landed point is the body's rect grown
    /// by it.
    fn landed(ui: &mut Ui, id: WidgetId, asked: Anchor, margin: Spacing) -> Anchor {
        if !asked.is_point() {
            return asked;
        }
        let held = ui
            .state::<Option<Landing>>(id)
            .copied()
            .flatten()
            .filter(|landing| landing.asked == asked);
        let landed = match held {
            Some(Landing {
                landed: Some(point),
                ..
            }) => Some(point),
            // The popup recorded at `asked` last frame, so its rect is where that frame landed it.
            Some(Landing { landed: None, .. }) => ui
                .response_for(id)
                .rect
                .map(|rect| rect.inflated_by(margin).min),
            None => None,
        };
        ui.with_state::<Option<Landing>, _>(id, |_, landing| {
            *landing = Some(Landing { asked, landed });
        });
        landed.map_or(asked, Anchor::at_point)
    }

    /// The full-surface click-eater under the popup `id`, which takes a press outside its body.
    pub(crate) fn eater_id(id: WidgetId) -> WidgetId {
        id.with("eater")
    }

    /// Dress the popup as a menu panel: the menu theme's chrome, padding, row gap and minimum
    /// width, each unless the caller set it, and arrow keys walking the rows.
    pub(crate) fn menu_look(self, menu: &MenuTheme) -> Self {
        self.default_chrome(menu.panel.clone())
            .default_padding(menu.padding)
            .default_min_size(Size::new(menu.min_width, 0.0))
            .default_gap(menu.gap)
            .arrow_focus(Axis::Y)
    }

    /// Paint `chrome` behind this widget; unset falls back to [`Theme::popup`]'s panel, as the
    /// padding does. [`Chrome::NONE`] suppresses the fallback.
    ///
    /// [`Theme::popup`]: crate::Theme::popup
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

    /// Paint `chrome` unless the caller set one; an explicit
    /// [`Self::chrome`] wins in either order. For wrappers that theme a held
    /// widget after the caller's setters ran.
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

impl Configure for Popup<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
