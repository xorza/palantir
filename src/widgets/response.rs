//! What a widget hands back: the lazy interaction handle, its owned
//! snapshot, and the body-value pairing for widgets that take a closure.

use crate::input::response::response_state::ResponseState;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use std::cell::OnceCell;

/// Lazy handle to a widget's per-frame interaction state. Holds a
/// `WidgetId` plus a shared borrow of `Ui`; the first deref probes
/// `ui.response_for(self.id)` and memoizes the result. Dropping the
/// handle without touching it skips the probe entirely — the common
/// case for decorative widgets (Text, Block, Panel chrome, etc.).
///
/// There is **no accessor surface of its own**: `Response` derefs to
/// [`ResponseState`], so everything reads exactly like the state —
/// `r.hovered()`, `r.pressed()`, `r.clicked()`,
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
///
/// # Reaching one out of a richer result
///
/// A widget that owes the caller more than interaction returns a wrapper
/// holding this in a `response` field — [`InnerResponse`],
/// [`ValueResponse`](crate::ValueResponse),
/// [`SelectResponse`](crate::SelectResponse),
/// [`TextEditResponse`](crate::TextEditResponse) and the rest. **None of
/// them derefs to it**, so interaction is always spelled
/// `r.response.clicked()` there where a plain response spells
/// `r.clicked()`.
///
/// That is deliberate, and pinned by a test. A wrapper carries two things
/// a call site has to keep apart — what the body returned, or what the
/// value did, beside what the pointer did. Deref would let either be
/// read without saying which was meant. This type derefs to
/// [`ResponseState`] because it *is* a handle to one; a wrapper merely
/// contains a response, and says so.
pub struct Response<'a> {
    /// Widget id of the originating widget. Stable across frames as
    /// long as the call-site / explicit-key inputs don't change.
    /// Cheap — reading it never probes.
    pub id: WidgetId,
    ui: &'a Ui,
    /// `OnceCell` so `deref` can lend `&ResponseState` out of the
    /// lazily-filled cache. The state survives later reads — a
    /// `Tooltip` / `Scroll` body that asks for `hovered()`, `pressed()`,
    /// and `drag_delta()` in sequence pays for exactly one
    /// `response_for` probe.
    cached: OnceCell<ResponseState>,
}

impl<'a> Response<'a> {
    /// Empty-cache constructor — the first deref triggers
    /// `response_for`. Used by widgets that don't otherwise consume
    /// the response state during `.show()` (decorative widgets:
    /// Text, Block, Panel, Grid). External widget authors reach this
    /// through [`Widget::response`](crate::widget::Widget::response).
    #[inline]
    pub(super) fn lazy(id: WidgetId, ui: &'a Ui) -> Self {
        Self {
            id,
            ui,
            cached: OnceCell::new(),
        }
    }

    /// Pre-filled-cache constructor — bypasses the first-deref probe by
    /// handing in the already-known `ResponseState`.
    ///
    /// **The closing half of the eager path.** An interactive widget
    /// needs its response before it records — theme picking reads it,
    /// and a value-writing widget acts on it — so it probes once through
    /// `Widget::response`, carries the owned state across `record`, and
    /// hands it back here rather than letting the caller re-probe. `Widget::show` packages
    /// the lazy path for widgets that need none of that.
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
    /// r.clicked() {…}`. The cache fills on first deref either
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
    /// The state as it stood when the snapshot was taken. Also reached
    /// through this type's [`Deref`](std::ops::Deref).
    pub state: ResponseState,
}

impl std::ops::Deref for ResponseSnapshot {
    type Target = ResponseState;
    #[inline]
    fn deref(&self) -> &ResponseState {
        &self.state
    }
}

/// [`Response`] plus a value returned by the body closure of widgets
/// that take one (`Panel`/`Grid`/`Scroll`). Interaction state is
/// available through [`Self::response`]; the body result is available
/// through [`Self::inner`].
#[derive(Debug)]
pub struct InnerResponse<'a, R> {
    /// The container's own pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// What the body returned.
    pub inner: R,
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::widgets::response::Response;

    impl Response<'_> {
        pub(crate) fn node(&self) -> NodeId {
            self.ui.forest().node_for_widget_id(Layer::Main, self.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::expander::ExpanderResponse;
    use crate::widgets::response::InnerResponse;
    use crate::widgets::select_response::SelectResponse;
    use crate::widgets::tabs::tab_strip::TabStripResponse;
    use crate::widgets::tabs::tabbed_view::TabbedViewResponse;
    use crate::widgets::text_edit::TextEditResponse;
    use crate::widgets::value_response::ValueResponse;
    use static_assertions::assert_not_impl_any;
    use std::ops::Deref;

    // Pins the rule `Response`'s own doc states, under "Reaching one out
    // of a richer result" — every wrapper, so the doc's "none of them"
    // cannot drift into "three of them".
    assert_not_impl_any!(InnerResponse<'static, ()>: Deref);
    assert_not_impl_any!(ValueResponse<'static>: Deref);
    assert_not_impl_any!(SelectResponse<'static>: Deref);
    assert_not_impl_any!(TextEditResponse<'static>: Deref);
    assert_not_impl_any!(ExpanderResponse<'static, ()>: Deref);
    assert_not_impl_any!(TabStripResponse<'static>: Deref);
    assert_not_impl_any!(TabbedViewResponse<'static>: Deref);
}
