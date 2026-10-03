//! A popup a trigger widget opens and closes by clicking.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::close_handle::CloseHandle;
use crate::widgets::popup::Popup;

/// A [`Popup`] dropped below a trigger: a click on the trigger opens it, a
/// second click, an outside press, Escape or a [`CloseHandle`] closes it.
///
/// The whole open/close protocol of a dropdown control in one place — a
/// [`ComboBox`](crate::ComboBox), a [`ColorButton`](crate::ColorButton), or
/// an app's own panel that drops from a button:
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
/// [`ContextMenu`](crate::ContextMenu)'s does, and is written back only on
/// the frame it flips — a closed trigger, nearly every frame, keeps no row.
/// A disabled trigger closes its popup, as a native one does: the popup is
/// a tree of its own, and would go on taking input for a control that
/// refuses it.
///
/// Implements [`Configure`], forwarding to the popup, so `.id(...)`,
/// `.min_size(...)` and `.padding(...)` shape the panel. Its identity
/// defaults to one derived from the trigger's.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct PopupTrigger {
    for_id: WidgetId,
    /// The trigger's state on the frame the snapshot was taken: its click
    /// toggles the popup, its rect anchors it, and its disabled flag
    /// closes it.
    trigger: ResponseState,
    /// The popup this trigger drops. Its anchor is a placeholder until
    /// `show` re-anchors it below the trigger's rect.
    popup: Popup,
}

/// Open/closed flag for one trigger, keyed off the trigger's id.
#[derive(Default, Clone, Copy, Debug)]
struct PopupTriggerState {
    open: bool,
}

impl PopupTrigger {
    /// Attach a popup to the trigger `snapshot` was taken from. Pass via
    /// `trigger.snapshot()` to detach from the trigger's `&Ui` borrow.
    pub fn on(snapshot: &ResponseSnapshot) -> Self {
        Self {
            for_id: snapshot.id,
            trigger: snapshot.state,
            popup: Popup::new(Anchor::below(Rect::ZERO)).default_id(snapshot.id.with("popup")),
        }
    }

    /// Paint `bg` as the popup's background. See [`Popup::background`].
    pub const fn background(mut self, bg: Background) -> Self {
        self.popup = self.popup.background(bg);
        self
    }

    /// Paint `bg` as the popup's background unless the caller set one. See
    /// [`Popup::default_background`].
    pub const fn default_background(mut self, bg: Background) -> Self {
        self.popup = self.popup.default_background(bg);
        self
    }

    /// Toggle the open flag on a trigger click, close it on a disabled
    /// trigger, and record the popup below the trigger while it is open.
    ///
    /// The popup's own [`OverlayResponse`], as
    /// [`ContextMenu::show`](crate::ContextMenu::show) returns it: its
    /// `inner` is `None` on a frame the popup is closed and the body does
    /// not run.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        let was_open = Self::is_open(ui, self.for_id);
        let mut open = was_open;
        if self.trigger.clicked() {
            open = !open;
        }
        if self.trigger.disabled {
            open = false;
        }
        let mut resp = OverlayResponse::default();
        if open && let Some(rect) = self.trigger.rect {
            resp = self
                .popup
                .anchored(Anchor::below(rect))
                .show(ui, |ui, handle| Some(body(ui, handle)));
            if resp.closed() {
                open = false;
            }
        }
        if open != was_open {
            ui.state_or_default::<PopupTriggerState>(self.for_id).open = open;
        }
        resp
    }

    /// Open the popup of the trigger `for_id`, for a programmatic open — a
    /// keyboard shortcut. It records on that trigger's next `show`.
    pub fn open(ui: &mut Ui, for_id: WidgetId) {
        ui.state_or_default::<PopupTriggerState>(for_id).open = true;
    }

    /// Close the popup of the trigger `for_id`. No-op if already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        if let Some(state) = ui.state_mut::<PopupTriggerState>(for_id) {
            state.open = false;
        }
    }

    /// `true` while the popup of the trigger `for_id` is open. A probe: no
    /// row is allocated for a trigger that has never been opened.
    pub fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        ui.state::<PopupTriggerState>(for_id)
            .is_some_and(|state| state.open)
    }
}

/// Forwards to the popup this trigger drops, so `.min_size(...)` and the
/// rest shape the panel.
impl Configure for PopupTrigger {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.popup.configure()
    }
}

#[cfg(test)]
mod tests;
