use crate::forest::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::Response;

/// A simple decorated rectangle: optional background / size / margin
/// plus an optional `Sense`. Used directly for dividers / hit-areas /
/// bg swatches, and as the rendering primitive inside `Button`.
/// Chrome + clip behavior come from
/// [`Configure::background`] / [`Configure::clip_rect`] /
/// [`Configure::clip_rounded`].
pub struct Frame {
    element: Element,
}

impl Frame {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::new(LayoutMode::Leaf),
        }
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let node = ui.node(self.element, |_| {});
        let state = ui.response_for(id);
        Response { node, id, state }
    }
}

impl Configure for Frame {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
