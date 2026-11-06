//! The body-value pairing of what a widget hands back.

use crate::primitives::input::interaction::response::Response;

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
    use crate::primitives::input::interaction::response::Response;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node_id::NodeId;
    use crate::ui::Ui;

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
