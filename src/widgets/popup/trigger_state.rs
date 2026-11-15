//! The open state an overlay trigger keeps between frames, and the frame that drives it: one
//! protocol under [`PopupTrigger`](crate::PopupTrigger) and [`Menu`](crate::Menu).

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::layout::anchor::Anchor;
use crate::ui::Ui;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widgets::popup::Popup;
use glam::Vec2;
use std::marker::PhantomData;

/// Where an open trigger's popup sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Opening {
    /// Below the trigger's rect, as it stands each frame.
    Below,
    /// At a surface point, fixed when it opened.
    At(Vec2),
}

/// One trigger's overlay: whether it is open, and the popup it showed last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TriggerState {
    open: Option<Opening>,
    /// The popup recorded for it last frame: its input scope is what a close gives back, since a
    /// popup that merely stops recording would own input one frame more.
    shown: Option<WidgetId>,
}

impl TriggerState {
    /// A state opened at `opening`, for an owner that keeps it rather than the trigger's row.
    pub(crate) const fn opened(opening: Opening) -> Self {
        Self {
            open: Some(opening),
            shown: None,
        }
    }

    /// Whether the popup is open.
    pub(crate) const fn is_open(self) -> bool {
        self.open.is_some()
    }

    /// Give back the input scope this state's popup held, as it closes.
    pub(crate) fn release(self, ui: &mut Ui) {
        if let Some(popup) = self.shown {
            ui.withdraw_input_scope(popup);
        }
    }
}

/// A trigger's [`TriggerState`] in the state map, keyed off the trigger's id. `K` is the widget
/// that owns the row, so a trigger a [`PopupTrigger`](crate::PopupTrigger) drops a panel from and
/// a [`Menu`](crate::Menu) raises a context menu on keeps two. A trigger has one menu row:
/// [`Menu::context`](crate::Menu::context) and [`Menu::below`](crate::Menu::below) on one trigger
/// share it, and [`Menu::menu_id`](crate::Menu::menu_id) with it.
#[derive(Debug)]
pub(crate) struct TriggerRow<K>(TriggerState, PhantomData<fn() -> K>);

impl<K> Default for TriggerRow<K> {
    fn default() -> Self {
        Self(TriggerState::default(), PhantomData)
    }
}

impl<K: 'static> TriggerRow<K> {
    /// The state of `for_id`, a closed one when there is no row; allocates nothing.
    fn of(ui: &Ui, for_id: WidgetId) -> TriggerState {
        ui.state::<Self>(for_id)
            .map_or_else(TriggerState::default, |row| row.0)
    }

    pub(crate) fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        Self::of(ui, for_id).is_open()
    }

    pub(crate) fn open(ui: &mut Ui, for_id: WidgetId, opening: Opening) {
        ui.with_state::<Self, _>(for_id, |_, row| row.0.open = Some(opening));
    }

    /// Close the popup of `for_id` and give back the input scope it held; a closed one stores
    /// nothing.
    pub(crate) fn close(ui: &mut Ui, for_id: WidgetId) {
        let state = Self::of(ui, for_id);
        if !state.is_open() {
            return;
        }
        state.release(ui);
        ui.with_state::<Self, _>(for_id, |_, row| row.0 = TriggerState::default());
    }
}

/// What [`TriggerFrame::settle`] leaves: the popup's outcome, `None` while closed, and the state
/// to keep.
#[derive(Debug)]
pub(crate) struct Settled<R> {
    pub(crate) response: Option<OverlayResponse<R>>,
    pub(crate) state: TriggerState,
}

/// One frame of a trigger's overlay: what the trigger did this frame, and where it stands.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TriggerFrame {
    pub(crate) for_id: WidgetId,
    /// A click on the trigger flips it, opening below.
    pub(crate) toggled: bool,
    /// The trigger opened it this frame, as a right-click opens a menu at the pointer.
    pub(crate) opened: Option<Opening>,
    /// A disabled trigger closes its popup, a tree of its own that would keep taking input.
    pub(crate) disabled: bool,
    /// What [`Opening::Below`] hangs from.
    pub(crate) rect: Option<Rect>,
}

impl TriggerFrame {
    /// The frame of a popup dropped below `trigger`, as a dropdown's is: the trigger's click
    /// toggles it and its rect anchors it.
    pub(crate) const fn below(trigger: &Response) -> Self {
        Self {
            for_id: trigger.id,
            toggled: trigger.clicked(),
            opened: None,
            disabled: trigger.disabled,
            rect: trigger.rect,
        }
    }

    /// Settle the open state, record `popup` with `body` while open, and keep the row. Every close
    /// gives the popup's input scope back: one from inside withdraws it there, one by the trigger
    /// here.
    pub(crate) fn show<'p, K: 'static, R>(
        self,
        ui: &mut Ui,
        popup: impl FnOnce(&Ui) -> Popup<'p>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Option<OverlayResponse<R>> {
        let for_id = self.for_id;
        let state = TriggerRow::<K>::of(ui, for_id);
        let settled = self.settle(ui, state, popup, body);
        // Written only on a change, so a trigger never opened mints no row.
        if settled.state != state {
            ui.with_state::<TriggerRow<K>, _>(for_id, |_, row| row.0 = settled.state);
        }
        settled.response
    }

    /// [`Self::show`] over a `state` its owner keeps rather than the trigger's row, as a menu bar
    /// keeps one for the one menu it has open; the state to keep comes back.
    pub(crate) fn settle<'p, R>(
        self,
        ui: &mut Ui,
        state: TriggerState,
        popup: impl FnOnce(&Ui) -> Popup<'p>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> Settled<R> {
        let mut open = state.open;
        if self.toggled {
            open = match open {
                Some(_) => None,
                None => Some(Opening::Below),
            };
        }
        if let Some(opening) = self.opened {
            open = Some(opening);
        }
        if self.disabled {
            open = None;
        }
        let anchor = match open {
            Some(Opening::Below) => self.rect.map(Anchor::below),
            Some(Opening::At(point)) => Some(Anchor::at_point(point)),
            None => None,
        };
        let mut resp = None;
        let mut shown = None;
        if let Some(anchor) = anchor {
            // Built only while open, so a closed trigger dresses no popup.
            let shown_now = popup(ui).anchor(anchor).show(ui, body);
            if shown_now.closed() {
                open = None;
            } else {
                shown = Some(shown_now.id);
            }
            resp = Some(shown_now);
        } else if let Some(popup) = state.shown {
            ui.withdraw_input_scope(popup);
        }
        Settled {
            response: resp,
            state: TriggerState { open, shown },
        }
    }
}
