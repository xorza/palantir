//! The open state an overlay trigger keeps between frames, and the frame that drives it: one
//! protocol under [`PopupTrigger`](crate::PopupTrigger) and [`ContextMenu`](crate::ContextMenu).

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::ui::Ui;
use crate::widget_core::overlay_response::OverlayResponse;
use crate::widgets::close_handle::CloseHandle;
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

/// One trigger's overlay in the state map, keyed off the trigger's id. `K` is the widget that
/// owns the row, so a trigger holding both a dropdown and a context menu keeps two.
#[derive(Debug)]
pub(crate) struct TriggerState<K> {
    open: Option<Opening>,
    /// The popup recorded for it last frame: its input scope is what a close gives back, since a
    /// popup that merely stops recording would own input one frame more.
    shown: Option<WidgetId>,
    kind: PhantomData<fn() -> K>,
}

impl<K> Default for TriggerState<K> {
    fn default() -> Self {
        Self {
            open: None,
            shown: None,
            kind: PhantomData,
        }
    }
}

impl<K> Clone for TriggerState<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for TriggerState<K> {}

impl<K> PartialEq for TriggerState<K> {
    fn eq(&self, other: &Self) -> bool {
        self.open == other.open && self.shown == other.shown
    }
}

impl<K: 'static> TriggerState<K> {
    /// The row of `for_id`, a closed one when there is none; allocates nothing.
    fn of(ui: &Ui, for_id: WidgetId) -> Self {
        ui.state::<Self>(for_id).copied().unwrap_or_default()
    }

    pub(crate) fn is_open(ui: &Ui, for_id: WidgetId) -> bool {
        Self::of(ui, for_id).open.is_some()
    }

    pub(crate) fn open(ui: &mut Ui, for_id: WidgetId, opening: Opening) {
        ui.with_state::<Self, _>(for_id, |_, state| state.open = Some(opening));
    }

    /// Close the popup of `for_id` and give back the input scope it held; a closed one stores
    /// nothing.
    pub(crate) fn close(ui: &mut Ui, for_id: WidgetId) {
        let state = Self::of(ui, for_id);
        if state.open.is_none() {
            return;
        }
        state.shut(ui);
        ui.with_state::<Self, _>(for_id, |_, state| *state = Self::default());
    }

    /// A state opened at `opening`, for an owner that keeps it rather than the trigger's row.
    pub(crate) const fn opened(opening: Opening) -> Self {
        Self {
            open: Some(opening),
            shown: None,
            kind: PhantomData,
        }
    }

    /// Whether the popup is open.
    pub(crate) const fn is_open_now(self) -> bool {
        self.open.is_some()
    }

    /// Give back the input scope this state's popup held, as it closes.
    pub(crate) fn shut(self, ui: &mut Ui) {
        if let Some(popup) = self.shown {
            ui.release_input_scope(popup);
        }
    }
}

/// What [`TriggerFrame::settle`] leaves: the popup's outcome and the state to keep.
#[derive(Debug)]
pub(crate) struct Settled<K, R> {
    pub(crate) response: OverlayResponse<Option<R>>,
    pub(crate) state: TriggerState<K>,
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
    /// Settle the open state, record `popup` with `body` while open, and keep the row.
    ///
    /// A popup opened while focus shows the ring (the keyboard opened it) takes focus on its first
    /// stop, as WAI-ARIA's menu button does; focus returns to the trigger on close. Every close
    /// gives the popup's input scope back: one from inside withdraws it there, one by the trigger
    /// here.
    pub(crate) fn show<K: 'static, R>(
        self,
        ui: &mut Ui,
        popup: Popup,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> OverlayResponse<Option<R>> {
        let for_id = self.for_id;
        let state = TriggerState::<K>::of(ui, for_id);
        let settled = self.settle(ui, state, popup, body);
        if settled.state != state {
            ui.with_state::<TriggerState<K>, _>(for_id, |_, row| *row = settled.state);
        }
        settled.response
    }

    /// [`Self::show`] over a `state` its owner keeps rather than the trigger's row, as a menu bar
    /// keeps one for the one menu it has open; the state to keep comes back.
    pub(crate) fn settle<K, R>(
        self,
        ui: &mut Ui,
        state: TriggerState<K>,
        popup: Popup,
        body: impl FnOnce(&mut Ui, &CloseHandle) -> R,
    ) -> Settled<K, R> {
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
        let mut resp = OverlayResponse::default();
        let mut shown = None;
        if let Some(anchor) = anchor {
            resp = popup
                .anchor(anchor)
                .show(ui, |ui, handle| Some(body(ui, handle)));
            if state.shown.is_none() && ui.is_focus_visible() {
                ui.focus_first_within(resp.id);
            }
            if resp.closed() {
                open = None;
            } else {
                shown = Some(resp.id);
            }
        } else if let Some(popup) = state.shown {
            ui.release_input_scope(popup);
        }
        Settled {
            response: resp,
            state: TriggerState {
                open,
                shown,
                kind: PhantomData,
            },
        }
    }
}
