pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod combo_box;
pub(crate) mod context_menu;
pub(crate) mod drag_value;
pub(crate) mod frame;
pub(crate) mod gpu_view;
pub(crate) mod grid;
pub(crate) mod modal;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod progress_bar;
pub(crate) mod radio;
pub(crate) mod scroll;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod switch;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod toggle;
pub(crate) mod tooltip;

use crate::forest::element::Element;

use crate::input::response::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;

use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;

use std::cell::OnceCell;

/// Resolve a container widget's chrome + clip against the theme
/// fallbacks, mutating `element`'s clip mode in place. Shared by
/// `Panel`/`Grid`/`Popup` (theme slot `panel_background` / `panel_clip`):
/// an explicit `.background(...)` wins, otherwise the theme default
/// fills in; the clip default only applies when the caller left clip at
/// [`ClipMode::None`]. Returns the chrome to pass to
/// [`Ui::node`].
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

/// Per-frame entry probe shared by interactive widgets
/// (`Button`/`Checkbox`/`RadioButton`): resolve the element's stable
/// [`WidgetId`] and probe its response exactly once. `raw` is the
/// un-merged state for the returned [`Response::eager`]; `merged` has
/// `Element::disabled` OR-ed in so a freshly toggled `.disabled(true)`
/// paints disabled visuals on the same frame — the cascade lags by one.
/// Centralizes the disabled-merge that the three widgets previously
/// hand-mirrored.
pub(crate) struct WidgetEntry {
    pub(crate) id: WidgetId,
    pub(crate) raw: ResponseState,
    pub(crate) merged: ResponseState,
}

pub(crate) fn enter_widget(ui: &mut Ui, element: &Element) -> WidgetEntry {
    let id = ui.widget_id(element);
    let raw = ui.response_for(id);
    let mut merged = raw;
    merged.disabled |= element.flags.is_disabled();
    WidgetEntry { id, raw, merged }
}

/// Lazy handle to a widget's per-frame interaction state. Holds a
/// `WidgetId` plus a shared borrow of `Ui`; the first deref probes
/// `ui.response_for(self.id)` and memoizes the result. Dropping the
/// handle without touching it skips the probe entirely — the common
/// case for decorative widgets (Text, Frame, Panel chrome, etc.).
///
/// There is **no accessor surface of its own**: `Response` derefs to
/// [`ResponseState`], so everything reads exactly like the state —
/// `r.hovered`, `r.pressed()`, `r.left.clicked()`,
/// `r.left.drag.delta()`, `r.scroll.pixels`. One API, defined once.
/// Deref-copy (`*r`) hands out the owned `Copy` state.
///
/// Widgets that already had to call `ui.response_for(id)` for their
/// own theme-picking / interaction logic (Button, Checkbox, …) hand
/// the already-paid-for state to [`Response::eager`] so callers
/// inherit the cached result without a second probe.
///
/// To detach from the `&Ui` borrow (e.g. before calling another
/// `&mut Ui` op while still holding the state), use
/// [`Response::snapshot`] to materialize a [`ResponseSnapshot`].
pub struct Response<'a> {
    /// Widget id of the originating widget. Stable across frames as
    /// long as the call-site / explicit-key inputs don't change.
    /// Cheap — reading it never probes.
    pub id: WidgetId,
    pub(crate) ui: &'a Ui,
    /// `OnceCell` so `deref` can lend `&ResponseState` out of the
    /// lazily-filled cache. The state survives later reads — a
    /// `Tooltip` / `Scroll` body that asks for `hovered`, `pressed()`,
    /// and `drag_delta()` in sequence pays for exactly one
    /// `response_for` probe.
    pub(crate) cached: OnceCell<ResponseState>,
}

impl<'a> Response<'a> {
    /// Empty-cache constructor — the first deref triggers
    /// `response_for`. Used by widgets that don't otherwise consume
    /// the response state during `.show()` (decorative widgets:
    /// Text, Frame, Panel, Grid).
    #[inline]
    pub fn lazy(id: WidgetId, ui: &'a Ui) -> Self {
        Self {
            id,
            ui,
            cached: OnceCell::new(),
        }
    }

    /// Pre-filled-cache constructor — bypasses the first-deref
    /// probe by handing in the already-known `ResponseState`. Used
    /// by widgets that called `ui.response_for(id)` themselves (e.g.
    /// for theme picking) so the caller doesn't re-probe.
    #[inline]
    pub fn eager(id: WidgetId, ui: &'a Ui, state: ResponseState) -> Self {
        Self {
            id,
            ui,
            cached: OnceCell::from(state),
        }
    }

    /// Materialize the state into an owned [`ResponseSnapshot`],
    /// releasing the `&Ui` borrow. Use this before any `&mut Ui` op
    /// that needs to interleave with reads from this response — e.g.
    /// `let r = btn.show(ui).snapshot(); …other_widget.show(ui); if
    /// r.left.clicked() {…}`. The cache fills on first deref either
    /// way, so this is purely a borrow-shape conversion.
    #[inline]
    pub fn snapshot(&self) -> ResponseSnapshot {
        ResponseSnapshot {
            id: self.id,
            state: **self,
        }
    }
}

impl std::ops::Deref for Response<'_> {
    type Target = ResponseState;
    /// The lazy probe: first touch resolves `response_for`, later
    /// touches read the memoized state.
    #[inline]
    fn deref(&self) -> &ResponseState {
        self.cached.get_or_init(|| self.ui.response_for(self.id))
    }
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
/// produces. Same deref surface as [`Response`] but doesn't borrow `Ui`,
/// so it can be stored across `&mut Ui` operations and passed to
/// consumers like [`crate::Tooltip::on`] / [`crate::ContextMenu::attach`]
/// that need a stable trigger anchor.
#[derive(Debug, Clone, Copy)]
pub struct ResponseSnapshot {
    /// Widget id of the originating widget.
    pub id: WidgetId,
    pub state: ResponseState,
}

impl std::ops::Deref for ResponseSnapshot {
    type Target = ResponseState;
    #[inline]
    fn deref(&self) -> &ResponseState {
        &self.state
    }
}

/// `Response` plus a value returned by the body closure of widgets
/// that take one (`Panel`/`Grid`/`Scroll`). `Deref`s to `Response` —
/// which itself derefs on to [`ResponseState`] — so callers ignoring
/// the inner value keep `panel.show(ui, body).left.clicked()` working
/// unchanged.
///
/// Constraint that keeps the `Deref` chain honest: **no inherent
/// methods or extra fields on `InnerResponse`** beyond `response` /
/// `inner` — a member named like anything on `Response` /
/// `ResponseState` would shadow it via the standard resolution order,
/// and callers would never see a compile error.
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
    use crate::forest::tree::NodeId;
    use crate::widgets::*;

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
