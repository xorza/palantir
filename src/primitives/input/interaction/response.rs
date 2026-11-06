//! [`Response`]: a widget's interaction state this frame, with its id.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response_state::ResponseState;
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
/// Wrappers ([`InnerResponse`](crate::InnerResponse), [`ValueResponse`](crate::ValueResponse),
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
