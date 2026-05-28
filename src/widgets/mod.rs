pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod context_menu;
pub(crate) mod frame;
pub(crate) mod grid;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod radio;
pub(crate) mod scroll;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod toggle;
pub(crate) mod tooltip;

use crate::forest::element::Element;
use crate::input::ResponseState;
use crate::input::pointer::PointerButton;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use glam::Vec2;
use std::cell::Cell;

/// Resolve a container widget's chrome + clip against the theme
/// fallbacks, mutating `element`'s clip mode in place. Shared by
/// `Panel`/`Grid`/`Popup` (theme slot `panel_background` / `panel_clip`):
/// an explicit `.background(...)` wins, otherwise the theme default
/// fills in; the clip default only applies when the caller left clip at
/// [`ClipMode::None`]. Returns the chrome to pass to
/// [`Ui::node_maybe_chrome`].
pub(crate) fn resolve_container_chrome(
    element: &mut Element,
    explicit: Option<Background>,
    theme_bg: Option<&Background>,
    theme_clip: ClipMode,
) -> Option<Background> {
    let chrome = explicit.or_else(|| theme_bg.cloned());
    if matches!(element.flags.clip_mode(), ClipMode::None) {
        element.flags.set_clip(theme_clip);
    }
    chrome
}

/// Lazy handle to a widget's per-frame interaction state. Holds a
/// `WidgetId` plus a shared borrow of `Ui`; reading any field probes
/// `ui.response_for(self.id)` on first access and memoizes the result.
/// Dropping the handle without reading any field skips the probe
/// entirely — the common case for decorative widgets (Text, Frame,
/// Panel chrome, etc.).
///
/// Widgets that already had to call `ui.response_for(id)` for their
/// own theme-picking / interaction logic (Button, Checkbox, …) hand
/// the already-paid-for state to [`Response::eager`] so callers
/// inherit the cached result without a second probe.
///
/// For multi-field reads or to detach from the `&Ui` borrow (e.g.
/// before calling another `&mut Ui` op while still holding the
/// state), use [`Response::snapshot`] to materialize a
/// [`ResponseSnapshot`].
/// Generates the shared read-only accessor surface for [`Response`]
/// and [`ResponseSnapshot`]. Both forward to a private
/// `resolved_state(&self) -> ResponseState` (lazy probe on `Response`,
/// owned field on `ResponseSnapshot`) and carry an `id` field, so the
/// accessor bodies are identical — the only real difference is how
/// `resolved_state` is obtained. Keep the two surfaces from drifting by
/// defining them once here.
macro_rules! response_accessors {
    () => {
        /// Widget id of the originating widget. Stable across frames as
        /// long as the call-site / explicit-key inputs don't change.
        /// Cheap — no `response_for` probe.
        #[inline]
        pub fn widget_id(&self) -> WidgetId {
            self.id
        }
        pub fn rect(&self) -> Option<Rect> {
            self.resolved_state().rect
        }
        /// Pre-transform layout rect — see
        /// [`crate::input::ResponseState::layout_rect`].
        pub fn layout_rect(&self) -> Option<Rect> {
            self.resolved_state().layout_rect
        }
        pub fn hovered(&self) -> bool {
            self.resolved_state().hovered
        }
        pub fn pressed(&self) -> bool {
            self.resolved_state().pressed
        }
        pub fn clicked(&self) -> bool {
            self.resolved_state().clicked
        }
        /// One-frame edge: right mouse button clicked-and-released on
        /// this widget without latching a drag. Independent of
        /// `clicked` (left).
        pub fn secondary_clicked(&self) -> bool {
            self.resolved_state().secondary_clicked
        }
        /// One-frame edge: any pointer button just double-clicked this
        /// widget (two clicks on the same id within
        /// [`crate::input::sense::DOUBLE_CLICK_WINDOW`]).
        pub fn double_clicked(&self) -> bool {
            self.resolved_state().double_clicked()
        }
        /// One-frame edge filtered by button.
        pub fn double_clicked_by(&self, button: PointerButton) -> bool {
            self.resolved_state().double_clicked_by(button)
        }
        /// Any button is currently dragging this widget.
        pub fn dragged(&self) -> bool {
            self.resolved_state().dragged()
        }
        /// `button` is currently dragging this widget.
        pub fn dragged_by(&self, button: PointerButton) -> bool {
            self.resolved_state().dragged_by(button)
        }
        /// One-frame edge: the active drag latched this frame. Snapshot
        /// the position here to anchor subsequent `drag_delta()` reads.
        pub fn drag_started(&self) -> bool {
            self.resolved_state().drag_started()
        }
        /// One-frame edge for `button`-drag specifically.
        pub fn drag_started_by(&self, button: PointerButton) -> bool {
            self.resolved_state().drag_started_by(button)
        }
        /// Cumulative pointer travel of the active drag (any button).
        /// `None` outside drag and for sub-threshold wiggle.
        pub fn drag_delta(&self) -> Option<Vec2> {
            self.resolved_state().drag_delta()
        }
        /// Cumulative pointer travel, filtered to `button`. `None` when
        /// a different button (or none) is dragging.
        pub fn drag_delta_by(&self, button: PointerButton) -> Option<Vec2> {
            self.resolved_state().drag_delta_by(button)
        }
        /// Pixel-precise scroll delta this frame, in logical pixels —
        /// the touchpad / precision-wheel source (winit
        /// `MouseScrollDelta::PixelDelta`). Routes only to widgets with
        /// [`crate::Sense::SCROLL`] that were the topmost scroll target
        /// under the pointer. `Vec2::ZERO` otherwise — and also when the
        /// widget *is* the target but no scroll event arrived this frame.
        /// Sign matches "advance offset forward" (positive = scroll
        /// down/right). Use for "trackpad pan" intent (e.g. a graph
        /// viewport that pans on touchpad and zooms on wheel).
        pub fn scroll_pixels(&self) -> Vec2 {
            self.resolved_state().scroll_pixels
        }
        /// Notched scroll delta this frame, in raw line units (NOT
        /// pixels) — the classic-wheel source (winit
        /// `MouseScrollDelta::LineDelta`). Same routing as
        /// [`Self::scroll_pixels`]. Use for "mouse wheel" intent (e.g.
        /// zoom-by-notches). To form a combined pan delta in pixels,
        /// compute `scroll_pixels() + scroll_lines() * line_px` where
        /// `line_px` is the caller's font-derived line step.
        pub fn scroll_lines(&self) -> Vec2 {
            self.resolved_state().scroll_lines
        }
        /// Multiplicative pinch zoom factor this frame (`1.0` = no
        /// pinch). Routes to widgets with [`crate::Sense::PINCH`].
        /// Independent of the scroll routes so a list can pan-via-scroll
        /// without committing to pinch-to-zoom, and vice versa.
        pub fn zoom_factor(&self) -> f32 {
            self.resolved_state().zoom_factor
        }
        /// Cursor position relative to this widget's `rect.min`. `None`
        /// when the pointer is off-surface or the widget didn't arrange.
        /// Useful as a pivot for zoom-about-cursor without recomputing
        /// the rect origin from `rect()`.
        pub fn pointer_local(&self) -> Option<Vec2> {
            self.resolved_state().pointer_local
        }
    };
}

pub struct Response<'a> {
    pub(crate) id: WidgetId,
    pub(crate) ui: &'a Ui,
    /// `Cell` so accessors can take `&self` and still update on
    /// first access. The cached `ResponseState` survives later reads
    /// — a `Tooltip` / `Scroll` body that asks for `hovered`,
    /// `pressed`, and `drag_delta` in sequence pays for exactly one
    /// `response_for` probe.
    pub(crate) cached: Cell<Option<ResponseState>>,
}

impl<'a> Response<'a> {
    /// Empty-cache constructor — first field access triggers
    /// `response_for`. Used by widgets that don't otherwise consume
    /// the response state during `.show()` (decorative widgets:
    /// Text, Frame, Panel, Grid).
    #[inline]
    pub(crate) fn lazy(id: WidgetId, ui: &'a Ui) -> Self {
        Self {
            id,
            ui,
            cached: Cell::new(None),
        }
    }

    /// Pre-filled-cache constructor — bypasses the first-access
    /// probe by handing in the already-known `ResponseState`. Used
    /// by widgets that called `ui.response_for(id)` themselves (e.g.
    /// for theme picking) so the caller doesn't re-probe.
    #[inline]
    pub(crate) fn eager(id: WidgetId, ui: &'a Ui, state: ResponseState) -> Self {
        Self {
            id,
            ui,
            cached: Cell::new(Some(state)),
        }
    }

    #[inline]
    fn resolved_state(&self) -> ResponseState {
        match self.cached.get() {
            Some(s) => s,
            None => {
                let s = self.ui.response_for(self.id);
                self.cached.set(Some(s));
                s
            }
        }
    }

    /// Materialize the full state into an owned [`ResponseSnapshot`],
    /// releasing the `&Ui` borrow. Use this before any `&mut Ui` op
    /// that needs to interleave with reads from this response — e.g.
    /// `let r = btn.show(ui).snapshot(); …other_widget.show(ui); if
    /// r.clicked() {…}`. The cache fills on first read either way,
    /// so this is purely a borrow-shape conversion, not a speed
    /// optimization for multi-field reads.
    #[inline]
    pub fn snapshot(&self) -> ResponseSnapshot {
        ResponseSnapshot {
            id: self.id,
            state: self.resolved_state(),
        }
    }

    response_accessors!();
}

impl std::fmt::Debug for Response<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("id", &self.id)
            .field("cached", &self.cached.get())
            .finish_non_exhaustive()
    }
}

/// Owned snapshot of a widget's response state — what [`Response::snapshot`]
/// produces. Carries the same accessor surface as [`Response`] but doesn't
/// borrow `Ui`, so it can be stored across `&mut Ui` operations and passed
/// to consumers like [`crate::Tooltip::for_`] / [`crate::ContextMenu::attach`]
/// that need a stable trigger anchor.
#[derive(Debug, Clone, Copy)]
pub struct ResponseSnapshot {
    pub(crate) id: WidgetId,
    pub(crate) state: ResponseState,
}

impl ResponseSnapshot {
    /// Owned form of the shared accessor contract — the snapshot's
    /// `state` is already materialized, so there's no probe to run.
    #[inline]
    fn resolved_state(&self) -> ResponseState {
        self.state
    }

    response_accessors!();
}

/// `Response` plus a value returned by the body closure of widgets
/// that take one (`Panel`/`Grid`/`Scroll`). `Deref`s to `Response` so
/// callers ignoring the inner value keep `panel.show(ui, body).clicked()`
/// working unchanged.
///
/// Three constraints keep the `Deref` shortcut honest. Breaking any
/// of them silently changes call-site behavior:
/// 1. **No inherent methods on `InnerResponse`** — a method named e.g.
///    `clicked` here would shadow `Response::clicked` via the standard
///    method-resolution order, and callers would never see a compile
///    error.
/// 2. **Field access doesn't auto-deref** — `r.response.id` works,
///    `r.id` does not. Don't extend `Response` with `pub` fields that
///    callers might expect to reach through `InnerResponse`.
/// 3. **`Response` methods stay `&self`-only** — `Deref::deref` yields
///    `&Response`, so any future `self`-consuming method on `Response`
///    would be unreachable via this shortcut. Callers would have to
///    write `r.response.foo()` instead of `r.foo()` — silent surface
///    drift.
#[derive(Debug)]
pub struct InnerResponse<'a, R> {
    pub response: Response<'a>,
    pub inner: R,
}

impl<'a, R> std::ops::Deref for InnerResponse<'a, R> {
    type Target = Response<'a>;
    fn deref(&self) -> &Response<'a> {
        &self.response
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code, private_interfaces)]
    use super::*;
    use crate::forest::tree::NodeId;

    impl Response<'_> {
        /// Old `Response.node` field as an inherent test-only method.
        pub fn node(&self) -> NodeId {
            self.ui.node_for_widget_id(self.id)
        }
    }

    impl ResponseSnapshot {
        /// Look up the node id given the widget id, for tests that
        /// hold a snapshot but still need to navigate the tree.
        pub fn node(&self, ui: &Ui) -> NodeId {
            ui.node_for_widget_id(self.id)
        }
    }
}

#[cfg(test)]
mod tests;
