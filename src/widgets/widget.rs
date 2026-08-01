//! The authoring handle a widget gets once its id is resolved, and the
//! entry probe interactive widgets build on top of it.

use crate::input::response::{ButtonPhase, ResponseState};
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::{InnerResponse, Response};

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

    /// Record this widget and hand back its lazy response paired with
    /// the body's value — the whole tail of a decorative widget's
    /// `show()`.
    ///
    /// The response is [`Self::response`], so a widget that never looks
    /// at it never pays for the `response_for` probe. Widgets returning
    /// a bare [`Response`] take `.response`; the ones with a body
    /// closure (`Panel`, `Grid`, `Scroll`) return the pair as-is.
    pub fn show<'a, R>(
        self,
        ui: &'a mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<'a, R> {
        let inner = self.record(ui, chrome, body);
        InnerResponse {
            response: self.response(ui),
            inner,
        }
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
    pub(super) widget: Widget,
    /// This frame's interaction state, `Node::disabled` folded in.
    ///
    /// Readable rather than behind an accessor because callers pass it
    /// alongside `&mut entry.widget.node` — a method would borrow all of
    /// `entry` and the two uses would conflict, and splitting them by
    /// copying costs a `ResponseState` memcpy per widget per frame.
    ///
    /// It is last frame's cascade snapshot: the only two things a widget
    /// legitimately knows that the snapshot cannot are
    /// [`Self::mark_focused`] and [`Self::mark_clicked`]. Writing here
    /// any other way is inventing input.
    pub(super) state: ResponseState,
    raw_disabled: bool,
}

impl WidgetEntry {
    pub(super) fn enter(ui: &mut Ui, node: Node) -> Self {
        let widget = ui.widget(node);
        let mut state = ui.response_for(widget.id());
        let raw_disabled = state.disabled;
        state.disabled |= widget.node.flags.is_disabled();
        Self {
            widget,
            state,
            raw_disabled,
        }
    }

    /// Report the widget as focused for the rest of this frame, after it
    /// called [`Ui::request_focus`] on itself.
    ///
    /// The snapshot predates the request — focus is resolved live, but
    /// this copy was taken on entry — so without this the widget's own
    /// response would deny the focus it just took.
    #[inline]
    pub(super) fn mark_focused(&mut self) {
        self.state.focused = true;
    }

    /// Report a single click, for a widget activated by something the
    /// pointer pipeline never saw — a keyboard shortcut bound to a menu
    /// row. Callers read `.clicked()` and must not have to care which
    /// device produced it.
    #[inline]
    pub(super) fn mark_clicked(&mut self) {
        self.state.left.phase = ButtonPhase::Up { click: Some(1) };
    }

    pub(super) fn into_response(mut self, ui: &Ui) -> Response<'_> {
        self.state.disabled = self.raw_disabled;
        Response::eager(self.widget.id(), ui, self.state)
    }
}
