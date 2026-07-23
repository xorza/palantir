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

use crate::scene::node::Node;

use crate::input::response::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;

use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;

use std::cell::OnceCell;

/// Resolve a container widget's chrome + clip against the theme
/// fallbacks, mutating `node`'s clip mode in place. Shared by
/// `Panel`/`Grid`/`Popup` (theme slot `panel_background` / `panel_clip`):
/// an explicit `.background(...)` wins, otherwise the theme default
/// fills in; the clip default only applies when the caller did not configure
/// clipping. Returns the chrome to pass to [`Ui::node`].
pub(crate) fn resolve_container_chrome(
    node: &mut Node,
    explicit: Option<Background>,
    theme_bg: Option<&Background>,
    theme_clip: ClipMode,
) -> Option<Background> {
    let chrome = explicit.or_else(|| theme_bg.cloned());
    node.clip.get_or_insert(theme_clip);
    chrome
}

/// A widget whose [`WidgetId`] has been resolved for this frame, paired
/// with the [`Node`] that id was resolved from — what [`Ui::widget`]
/// returns. This is the authoring primitive for widgets that need their
/// id *before* their node records: read last frame's interaction via
/// [`Ui::response_for`], pick themed chrome, mutate [`Self::node`],
/// derive child ids with `widget.id().with("child")` — then record with
/// [`Self::record`].
///
/// The id is read-only: it is the disambiguated identity the tree,
/// cascade, and `response_for` will see, and rebinding it would desync
/// them. The node stays open for mutation until `record` consumes it.
///
/// Record exactly once: resolution reserved this frame's occurrence
/// slot for the id and [`Self::record`] claims it. The type is `Copy`
/// (both halves are), so the compiler won't stop a second `record` call
/// — the frame will, with a duplicate-endpoint panic.
#[derive(Clone, Copy, Debug)]
#[must_use = "record the widget with Widget::record"]
pub struct Widget {
    id: WidgetId,
    pub node: Node,
}

impl Widget {
    pub(crate) fn new(id: WidgetId, node: Node) -> Self {
        Self { id, node }
    }

    /// The resolved, frame-disambiguated id — key for
    /// [`Ui::response_for`] / [`Ui::state_mut`] / [`Ui::animate`] and
    /// for deriving child ids via [`WidgetId::with`].
    #[inline]
    pub fn id(&self) -> WidgetId {
        self.id
    }

    /// Open this widget's node, run its body, and close it.
    ///
    /// `chrome` is `None` for the common layout-only / text-leaf /
    /// chrome-less path and `Some(bg)` when the widget paints a
    /// background — container widgets resolve an explicit-or-theme
    /// `Option<Background>` and pass `chrome.as_ref()`. Taken as
    /// `Option<&Background>` (an 8-byte niche-encoded pointer, not the
    /// 168 B `Background` by value) so the chrome travels as one pointer
    /// per hop down `Forest::open_node` → `Tree::open_node` →
    /// `shapes::lower::background`, and the no-chrome path is just a
    /// perfectly-predicted `None` branch.
    pub fn record<R>(
        self,
        ui: &mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        ui.node(self.id, self.node, chrome, body)
    }

    /// Lazy [`Response`] for this widget — the return value of choice
    /// for decorative widgets that never probed their state themselves.
    pub fn response<'a>(&self, ui: &'a Ui) -> Response<'a> {
        Response::lazy(self.id, ui)
    }
}

/// Per-frame entry probe shared by interactive widgets
/// (`Button`/`Checkbox`/`RadioButton`): resolve the node into a
/// [`Widget`] and probe its response exactly once. `state` has
/// `Node::disabled` OR-ed in for same-frame visuals and interaction;
/// [`Self::into_response`] restores the cascade snapshot's original
/// disabled bit for the returned [`Response::eager`].
#[derive(Debug)]
pub(crate) struct WidgetEntry {
    pub(crate) widget: Widget,
    pub(crate) state: ResponseState,
    raw_disabled: bool,
}

impl WidgetEntry {
    fn into_response(mut self, ui: &Ui) -> Response<'_> {
        self.state.disabled = self.raw_disabled;
        Response::eager(self.widget.id(), ui, self.state)
    }
}

pub(crate) fn enter_widget(ui: &mut Ui, node: Node) -> WidgetEntry {
    let widget = ui.widget(node);
    let mut state = ui.response_for(widget.id());
    let raw_disabled = state.disabled;
    state.disabled |= widget.node.flags.is_disabled();
    WidgetEntry {
        widget,
        state,
        raw_disabled,
    }
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
    /// Text, Frame, Panel, Grid). External widget authors reach this
    /// through [`Widget::response`].
    #[inline]
    pub(crate) fn lazy(id: WidgetId, ui: &'a Ui) -> Self {
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

#[cfg(test)]
pub(crate) mod test_support {
    use crate::scene::tree::node::NodeId;
    use crate::widgets::*;

    impl Response<'_> {
        pub(crate) fn node(&self) -> NodeId {
            self.ui.node_for_widget_id(self.id)
        }
    }
}
