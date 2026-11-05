//! What a widget hands back: its [`Response`], and the body-value pairing.

use crate::input::interaction::response_state::ResponseState;
use crate::primitives::identity::widget_id::WidgetId;
use std::ops;

/// A widget's interaction state this frame, probed once as the widget shows and owned: it
/// borrows nothing, so `&mut Ui` calls interleave with its reads, an overlay anchors to it
/// ([`Tooltip::on`](crate::Tooltip::on), [`Menu::context`](crate::Menu::context)), and a
/// composite gathers its children's.
///
/// It derefs to [`ResponseState`], so `r.hovered()`, `r.clicked()` etc. read like the state.
/// [`Ui::response_for`](crate::Ui::response_for) probes one by id.
///
/// Owned, not a lazy probe over `&Ui`: probing every widget's state as it shows costs 9 µs a
/// record pass of the frame bench's tree, and the borrow made an owned twin of each response
/// (AGENTS.md "A `Response` is owned").
///
/// # Reaching one out of a richer result
///
/// Wrappers ([`InnerResponse`], [`ValueResponse`](crate::ValueResponse),
/// [`TextEditResponse`](crate::TextEditResponse), ...) hold a `response`
/// field and **none derefs to it**: `r.response.clicked()`, not
/// `r.clicked()`. A wrapper carries a body or value result beside the
/// pointer state, and deref would blur which is read. Pinned by a test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    /// Widget id of the originating widget.
    pub id: WidgetId,
    /// The widget's state as it showed.
    pub state: ResponseState,
}

impl ops::Deref for Response {
    type Target = ResponseState;

    #[inline]
    fn deref(&self) -> &ResponseState {
        &self.state
    }
}

/// [`Response`] plus the value returned by the body closure of widgets that
/// take one (`Panel`/`Grid`/`Scroll`).
#[derive(Clone, Copy, Debug)]
pub struct InnerResponse<R> {
    /// The container's own pointer/click/hover [`Response`].
    pub response: Response,
    /// What the body returned.
    pub inner: R,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::ui::Ui;
    use crate::widget_core::response::Response;

    impl Response {
        pub(crate) fn node(&self, ui: &Ui) -> NodeId {
            ui.forest().node_for_widget_id(Layer::Main, self.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widget_core::response::InnerResponse;
    use crate::widget_core::value_response::ValueResponse;
    use crate::widgets::editable_label::EditableLabelResponse;
    use crate::widgets::expander::ExpanderResponse;
    use crate::widgets::tabs::tab_strip::TabStripResponse;
    use crate::widgets::tabs::tabbed_view::TabbedViewResponse;
    use crate::widgets::text_edit::TextEditResponse;
    use static_assertions::assert_not_impl_any;
    use std::ops::Deref;

    // Pins the rule in `Response`'s doc: no wrapper derefs to it.
    assert_not_impl_any!(InnerResponse<()>: Deref);
    assert_not_impl_any!(ValueResponse: Deref);
    assert_not_impl_any!(TextEditResponse: Deref);
    assert_not_impl_any!(ExpanderResponse<()>: Deref);
    assert_not_impl_any!(EditableLabelResponse: Deref);
    assert_not_impl_any!(TabStripResponse: Deref);
    assert_not_impl_any!(TabbedViewResponse<()>: Deref);
}
