//! What a widget hands back: the lazy [`Response`], its owned snapshot, and
//! the body-value pairing.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use std::cell::OnceCell;
use std::fmt;
use std::ops;

/// Lazy handle to a widget's per-frame interaction state. The first deref
/// probes `ui.response_for(id)` and memoizes; an untouched handle skips the
/// probe.
///
/// It derefs to [`ResponseState`], so `r.hovered()`, `r.clicked()` etc. read
/// like the state. Widgets that already probed pass their state to
/// [`Response::new`] to avoid a second probe. [`Response::snapshot`] detaches
/// from the `&Ui` borrow.
///
/// # Reaching one out of a richer result
///
/// Wrappers ([`InnerResponse`], [`ValueResponse`](crate::ValueResponse),
/// [`TextEditResponse`](crate::TextEditResponse), ...) hold a `response`
/// field and **none derefs to it**: `r.response.clicked()`, not
/// `r.clicked()`. A wrapper carries a body or value result beside the
/// pointer state, and deref would blur which is read. Pinned by a test.
pub struct Response<'a> {
    /// Widget id of the originating widget; reading it never probes.
    pub id: WidgetId,
    ui: &'a Ui,
    /// Filled on first deref.
    cached: OnceCell<ResponseState>,
}

impl<'a> Response<'a> {
    /// Empty-cache constructor; the first deref probes. For widgets that don't
    /// otherwise read the state (reached through [`Widget::show`](crate::widget::Widget::show)).
    #[inline]
    pub(crate) const fn lazy(id: WidgetId, ui: &'a Ui) -> Self {
        Self {
            id,
            ui,
            cached: OnceCell::new(),
        }
    }

    /// Pre-filled-cache constructor from an already-probed `state`, so the
    /// caller doesn't re-probe. For interactive widgets that need their
    /// response before recording.
    #[inline]
    pub fn new(id: WidgetId, ui: &'a Ui, state: ResponseState) -> Self {
        Self {
            id,
            ui,
            cached: OnceCell::from(state),
        }
    }

    /// Materialize the state into an owned [`ResponseSnapshot`], releasing the
    /// `&Ui` borrow so `&mut Ui` calls can interleave with later reads.
    #[inline]
    pub fn snapshot(&self) -> ResponseSnapshot {
        ResponseSnapshot {
            id: self.id,
            state: **self,
        }
    }
}

impl ops::Deref for Response<'_> {
    type Target = ResponseState;

    #[inline]
    fn deref(&self) -> &ResponseState {
        self.cached.get_or_init(|| self.ui.response_for(self.id))
    }
}

impl fmt::Debug for Response<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("id", &self.id)
            .field("cached", &self.cached.get())
            .finish_non_exhaustive()
    }
}

/// Owned snapshot of a response, from [`Response::snapshot`]. Derefs like
/// [`Response`] but doesn't borrow `Ui`, so it can anchor
/// [`crate::Tooltip::on`] / [`crate::ContextMenu::on`].
#[derive(Debug, Clone, Copy)]
pub struct ResponseSnapshot {
    /// Widget id of the originating widget.
    pub id: WidgetId,
    /// The state as it stood when the snapshot was taken.
    pub state: ResponseState,
}

impl ops::Deref for ResponseSnapshot {
    type Target = ResponseState;
    #[inline]
    fn deref(&self) -> &ResponseState {
        &self.state
    }
}

/// [`Response`] plus the value returned by the body closure of widgets that
/// take one (`Panel`/`Grid`/`Scroll`).
#[derive(Debug)]
pub struct InnerResponse<'a, R> {
    /// The container's own pointer/click/hover [`Response`].
    pub response: Response<'a>,
    /// What the body returned.
    pub inner: R,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::widget_core::response::Response;

    impl Response<'_> {
        pub(crate) fn node(&self) -> NodeId {
            self.ui.forest().node_for_widget_id(Layer::Main, self.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widget_core::response::InnerResponse;
    use crate::widget_core::value_response::ValueResponse;
    use crate::widgets::expander::ExpanderResponse;
    use crate::widgets::tabs::tab_strip::TabStripResponse;
    use crate::widgets::tabs::tabbed_view::TabbedViewResponse;
    use crate::widgets::text_edit::TextEditResponse;
    use static_assertions::assert_not_impl_any;
    use std::ops::Deref;

    // Pins the rule in `Response`'s doc: no wrapper derefs to it.
    assert_not_impl_any!(InnerResponse<'static, ()>: Deref);
    assert_not_impl_any!(ValueResponse<'static>: Deref);
    assert_not_impl_any!(TextEditResponse<'static>: Deref);
    assert_not_impl_any!(ExpanderResponse<'static, ()>: Deref);
    assert_not_impl_any!(TabStripResponse<'static>: Deref);
    assert_not_impl_any!(TabbedViewResponse<'static>: Deref);
}
