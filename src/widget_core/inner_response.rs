//! [`InnerResponse`]: a widget's response beside what its body returned.

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
