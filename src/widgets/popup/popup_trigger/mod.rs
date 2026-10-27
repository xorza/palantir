//! A popup a trigger widget opens and closes by clicking.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::layer::Layer;
use crate::theme::context_menu::ContextMenuTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::{Opening, TriggerFrame, TriggerState};

/// A [`Popup`] dropped below a trigger: a click opens it; a second click, an
/// outside press, Escape or a [`CloseHandle`] closes it. The whole protocol of a
/// dropdown control ([`ComboBox`](crate::ComboBox),
/// [`ColorButton`](crate::ColorButton), or an app's own panel):
///
/// ```
/// # use palantir::{Button, Configure, PopupTrigger, Text, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("Filters").show(ui).snapshot();
/// PopupTrigger::on(&trigger).show(ui, |ui, _| {
///     Text::new("…").show(ui);
/// });
/// # }
/// ```
///
/// Open state lives in the state map keyed off the trigger's id, as
/// [`ContextMenu`](crate::ContextMenu)'s does, and is written only on the frame
/// it flips. A disabled trigger closes its popup, since the popup is a tree of
/// its own and would keep taking input.
///
/// Implements [`Configure`], forwarding to the popup; identity defaults to one
/// derived from the trigger's.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct PopupTrigger {
    for_id: WidgetId,
    /// The trigger's state when the snapshot was taken: its click toggles the
    /// popup, its rect anchors it, its disabled flag closes it.
    trigger: ResponseState,
    /// The popup this trigger drops; its anchor is a placeholder until `show`.
    popup: Popup,
}

impl PopupTrigger {
    /// Attach a popup to the trigger `snapshot` was taken from.
    pub fn on(snapshot: &ResponseSnapshot) -> Self {
        Self {
            for_id: snapshot.id,
            trigger: snapshot.state,
            popup: Popup::new(Anchor::below(Rect::ZERO)).default_id(snapshot.id.with("popup")),
        }
    }

    /// Paint `chrome` behind the popup. See [`Popup::chrome`].
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        self.popup = self.popup.chrome(chrome);
        self
    }

    /// Paint `chrome` behind the popup unless the caller set one. See
    /// [`Popup::default_chrome`].
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        self.popup = self.popup.default_chrome(chrome);
        self
    }

    /// Record into `layer` rather than [`Layer::Popup`], as a menu button's menu does.
    pub(crate) const fn layer(mut self, layer: Layer) -> Self {
        self.popup = self.popup.layer(layer);
        self
    }

    /// Dress the popup as a menu panel, as [`ContextMenu`](crate::ContextMenu) is.
    pub(crate) fn menu_look(mut self, menu: &ContextMenuTheme) -> Self {
        self.popup = self.popup.menu_look(menu);
        self
    }

    /// Toggle the open flag on a trigger click, close it on a disabled trigger, and
    /// record the popup below the trigger while open.
    ///
    /// Returns the popup's [`OverlayResponse`] as
    /// [`ContextMenu::show`](crate::ContextMenu::show) does; `inner` is `None` while
    /// closed.
    ///
    /// A popup opened from the keyboard (Space or Enter on the focused trigger, or
    /// [`Self::open`] with keyboard focus) takes focus on its first stop, as
    /// WAI-ARIA's menu button does; a click leaves focus on the trigger. Focus
    /// returns to the trigger on close.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        TriggerFrame {
            for_id: self.for_id,
            toggled: self.trigger.clicked(),
            opened: None,
            disabled: self.trigger.disabled,
            rect: self.trigger.rect,
        }
        .show::<Self, R>(ui, self.popup, body)
    }

    /// Open the popup of the trigger `for_id`, e.g. from a keyboard shortcut; it
    /// records on that trigger's next `show`.
    pub fn open(ui: &mut Ui, for_id: WidgetId) {
        TriggerState::<Self>::open(ui, for_id, Opening::Below);
    }

    /// Close the popup of the trigger `for_id`, giving back the input it held at once. No-op if
    /// already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        TriggerState::<Self>::close(ui, for_id);
    }

    /// `true` while the popup of the trigger `for_id` is open. No row is allocated
    /// for a never-opened trigger.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        TriggerState::<Self>::is_open(ui, for_id)
    }
}

/// Forwards to the popup this trigger drops.
impl Configure for PopupTrigger {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.popup.configure()
    }
}

#[cfg(test)]
mod tests;
