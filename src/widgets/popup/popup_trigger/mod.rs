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

/// One trigger's popup, keyed off the trigger's id: whether it is open,
/// and whether it was on show last frame — so the frame it first shows is
/// known, however it was opened.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct PopupTriggerState {
    open: bool,
    shown: bool,
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

    /// Paint `background` as the popup's background. See [`Popup::background`].
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        self.popup = self.popup.background(background);
        self
    }

    /// Paint `background` as the popup's background unless the caller set one. See
    /// [`Popup::default_background`].
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        self.popup = self.popup.default_background(background);
        self
    }

    /// Toggle the open flag on a trigger click, close it on a disabled
    /// trigger, and record the popup below the trigger while it is open.
    ///
    /// The popup's own [`OverlayResponse`], as
    /// [`ContextMenu::show`](crate::ContextMenu::show) returns it: its
    /// `inner` is `None` on a frame the popup is closed and the body does
    /// not run.
    ///
    /// A popup opened from the keyboard — Space or Enter on a focused
    /// trigger, or [`Self::open`] while focus came from the keyboard —
    /// takes focus on its first stop as it shows, as WAI-ARIA's menu
    /// button does; one opened by a click leaves focus on the trigger.
    /// Focus goes back to the trigger when it closes.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        let state = ui
            .state::<PopupTriggerState>(self.for_id)
            .copied()
            .unwrap_or_default();
        let mut open = state.open;
        if self.trigger.clicked() {
            open = !open;
        }
        if self.trigger.disabled {
            open = false;
        }
        let mut resp = OverlayResponse::default();
        let mut shown = false;
        if open && let Some(rect) = self.trigger.rect {
            shown = true;
            resp = self
                .popup
                .anchor(Anchor::below(rect))
                .show(ui, |ui, handle| Some(body(ui, handle)));
            if !state.shown && ui.is_focus_visible() {
                ui.focus_first_within(resp.id);
            }
            if resp.closed() {
                open = false;
            }
        }
        let next = PopupTriggerState {
            open,
            shown: shown && open,
        };
        if next != state {
            ui.with_state::<PopupTriggerState, _>(self.for_id, |_, s| *s = next);
        }
        resp
    }

    /// Open the popup of the trigger `for_id`, for a programmatic open — a
    /// keyboard shortcut. It records on that trigger's next `show`.
    pub fn open(ui: &mut Ui, for_id: WidgetId) {
        ui.with_state::<PopupTriggerState, _>(for_id, |_, s| s.open = true);
    }

    /// Close the popup of the trigger `for_id`. No-op if already closed.
    pub fn close(ui: &mut Ui, for_id: WidgetId) {
        // Probed first, so closing a closed popup stores nothing.
        if ui
            .state::<PopupTriggerState>(for_id)
            .is_some_and(|s| s.open)
        {
            ui.with_state::<PopupTriggerState, _>(for_id, |_, s| s.open = false);
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
