//! A popup a trigger widget opens and closes by clicking.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::paint::chrome::Chrome;
use crate::theme::popup::PopupTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widgets::menu::menu_landing::MenuLanding;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::{Opening, TriggerFrame, TriggerState};

/// A [`Popup`] dropped below a trigger: a click opens it; a second click, an
/// outside press, Escape or a [`Ui::close_overlay`] in the body closes it. The whole protocol of a
/// dropdown control ([`ComboBox`](crate::ComboBox),
/// [`ColorButton`](crate::ColorButton), or an app's own panel):
///
/// ```
/// # use palantir::{Button, Configure, PopupTrigger, Text, Ui};
/// # fn demo(ui: &mut Ui) {
/// let trigger = Button::new().label("Filters").show(ui);
/// PopupTrigger::on(&trigger).show(ui, |ui| {
///     Text::new("…").show(ui);
/// });
/// # }
/// ```
///
/// Open state lives in the state map keyed off the trigger's id, as
/// [`Menu`](crate::Menu)'s does, and is written only on the frame
/// it flips. A disabled trigger closes its popup, since the popup is a tree of
/// its own and would keep taking input.
///
/// Implements [`Configure`], forwarding to the popup; its id defaults to
/// [`Self::popup_id`] of the trigger's.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct PopupTrigger<'a> {
    /// What the trigger did as it showed: its click toggles the popup, its rect anchors it, its
    /// disabled flag closes it.
    frame: TriggerFrame,
    /// The popup this trigger drops; anchored below the trigger at `show`.
    popup: Popup<'a>,
}

impl<'a> PopupTrigger<'a> {
    /// Attach a popup to the trigger whose response is `trigger`.
    pub fn on(trigger: &Response) -> Self {
        Self {
            frame: TriggerFrame::below(trigger),
            popup: Popup::new().default_id(PopupTrigger::popup_id(trigger.id)),
        }
    }

    /// Per-instance override of [`Theme::popup`](crate::Theme::popup). See [`Popup::style`].
    pub fn style(mut self, s: impl Into<Option<&'a PopupTheme>>) -> Self {
        self.popup = self.popup.style(s);
        self
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

    /// Toggle the open flag on a trigger click, close it on a disabled trigger, and
    /// record the popup below the trigger while open.
    ///
    /// Returns the popup's [`OverlayResponse`] as [`Menu::show`](crate::Menu::show) does, `None`
    /// while closed.
    ///
    /// A popup opened from the keyboard (Space or Enter on the focused trigger, or
    /// [`Self::open`] with keyboard focus) takes focus on its first stop, as
    /// WAI-ARIA's menu button does; a click leaves focus on the trigger. Focus returns to the
    /// trigger on close.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        self.show_as::<PopupTrigger<'static>, R>(ui, body)
    }

    /// [`Self::show`] with the open state in the row of `K`, the widget that wraps the trigger,
    /// so the row is reached through that widget's own `open`, `close` and `is_open` alone.
    pub(crate) fn show_as<K: 'static, R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        let resp = self.frame.show::<K, R>(ui, |_| self.popup, body);
        MenuLanding::First.land(ui, resp.as_ref());
        resp
    }
}

/// Driving a trigger's popup from outside, by the trigger's id, and the id of the popup.
impl PopupTrigger<'_> {
    /// The popup of the trigger `trigger`, unless its `Configure` names another.
    pub fn popup_id(trigger: WidgetId) -> WidgetId {
        trigger.with("popup")
    }

    /// Open the popup of the trigger `trigger`, e.g. from a keyboard shortcut; it
    /// records on that trigger's next `show`.
    pub fn open(ui: &mut Ui, trigger: WidgetId) {
        TriggerState::<PopupTrigger<'static>>::open(ui, trigger, Opening::Below);
    }

    /// Close the popup of the trigger `trigger`, giving back the input it held at once. No-op if
    /// already closed.
    pub fn close(ui: &mut Ui, trigger: WidgetId) {
        TriggerState::<PopupTrigger<'static>>::close(ui, trigger);
    }

    /// `true` while the popup of the trigger `trigger` is open. No row is allocated
    /// for a never-opened trigger.
    pub fn is_open(ui: &Ui, trigger: WidgetId) -> bool {
        TriggerState::<PopupTrigger<'static>>::is_open(ui, trigger)
    }
}

/// Forwards to the popup this trigger drops.
impl Configure for PopupTrigger<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.popup.configure()
    }
}

#[cfg(test)]
mod tests;
