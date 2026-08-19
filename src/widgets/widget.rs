//! The authoring handle a widget gets once its id is resolved.
//!
//! [`Ui::widget`] is the one resolver and [`Widget::record`] the one
//! opener: one resolution reserves one occurrence slot, and exactly one
//! `record` claims it. A widget that cannot know its final node at
//! resolution time overwrites [`Widget::node`] before recording — the
//! id is what must stay put.
//!
//! One handle, no companion type: [`Widget`] owns the node until it
//! records, and [`Widget::response`] hands back a plain owned
//! [`ResponseState`] that outlives it.

use crate::input::response::ResponseState;
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
/// slot for the id and [`Self::record`] claims it. **Deliberately not
/// `Copy`** even though both halves are — `record` takes `self`, so a
/// second call is a use-after-move the compiler rejects, rather than a
/// duplicate-endpoint panic at frame time.
#[derive(Debug)]
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

    /// Probe this frame's interaction state, folding in the node's own
    /// `disabled` bit so a widget disabled *this* frame reads and paints
    /// as disabled without waiting for the cascade to catch up.
    ///
    /// The eager half of the API, for widgets that act on input:
    /// decorative ones never call this and never pay for the
    /// `response_for` lookup — which is the whole reason the probe is a
    /// separate step rather than something [`Ui::widget`] always does.
    ///
    /// Returns a plain owned [`ResponseState`], not a borrowing
    /// [`Response`]: everything a widget does after probing — theme
    /// resolution, [`Self::record`], the body closure — needs
    /// `&mut Ui`, and a `Response` holds `&Ui` for its lazy cache.
    /// Owned state is what lets the probe outlive all of it and become
    /// the widget's [`Response::eager`] at the end.
    pub(crate) fn response(&self, ui: &Ui) -> ResponseState {
        let mut state = ui.response_for(self.id);
        state.disabled |= self.node.flags.is_disabled();
        state
    }

    /// Open this widget's node, run its body, and close it.
    ///
    /// **The crate's one opener.** Every widget reaches the tree here,
    /// so the open/close pairing lives in a single place.
    ///
    /// `chrome` is `None` for the common layout-only / text-leaf /
    /// chrome-less path and `Some(bg)` when the widget paints a
    /// background — container widgets resolve an explicit-or-theme
    /// `Option<Background>` and pass `chrome.as_ref()`. Both it and the
    /// node travel by reference from here down `Forest::open_node` →
    /// `Tree::open_node` → `Node::into_columns`, so neither the
    /// `Background` nor the `Node` is re-copied per hop.
    pub fn record<R>(
        self,
        ui: &mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        ui.forest.open_node(self.id, self.node, chrome);
        let r = body(ui);
        ui.forest.close_node();
        r
    }

    /// [`Self::record`] plus a lazy [`Response`] for the node just
    /// recorded — the whole tail of a decorative widget's `show()`.
    ///
    /// A convenience over the opener, not a second way to open:
    /// `record` is what every widget in the crate calls, and this is for
    /// the handful (`Frame`, `Panel`, `Grid`, `Separator`) whose `show()`
    /// is exactly "record, then hand the caller a response". Widgets
    /// returning a bare [`Response`] take `.response`; the ones with a
    /// body closure return the pair as-is.
    ///
    /// The response is **lazy**, so a caller that never reads it never
    /// pays for the `response_for` probe — which is why opening through
    /// `record` directly, as the other ~50 sites do, costs nothing extra
    /// over this and keeps `&Ui` unborrowed afterwards.
    ///
    /// **The eager path is the other one, and it has no packager.** Every
    /// interactive widget needs its response *before* recording — to pick
    /// a theme, to write a bound value — so it opens with
    /// `Widget::response` and closes with [`Response::eager`], with the
    /// probe in hand throughout — including `ToggleChrome::record_row`, which takes it
    /// once on behalf of the three toggles. There is deliberately no
    /// helper for it: `response` and `id` both come off the widget, and
    /// `id` has to be bound before `record` consumes it, so a packager
    /// would save one line and cost a type.
    pub fn show<'a, R>(
        self,
        ui: &'a mut Ui,
        chrome: Option<&Background>,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<'a, R> {
        // Read before `record` consumes the widget.
        let id = self.id;
        let inner = self.record(ui, chrome, body);
        InnerResponse {
            response: Response::lazy(id, ui),
            inner,
        }
    }
}
