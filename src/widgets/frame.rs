use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::Surface;

/// A simple decorated rectangle: optional background / size / margin
/// plus an optional `Sense`. Used directly for dividers / hit-areas /
/// bg swatches, and as the rendering primitive inside `Button`.
/// Background is `None` by default — Frame paints nothing unless one
/// is set via `.background(...)`.
pub struct Frame {
    element: Element,
    surface: Option<Surface>,
}

impl Frame {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_element(Element::new_auto(LayoutMode::Leaf))
    }

    pub fn for_element(element: Element) -> Self {
        Self {
            element,
            surface: None,
        }
    }

    pub fn background(mut self, s: impl Into<Surface>) -> Self {
        self.surface = Some(s.into());
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let node = ui.node(self.element, self.surface, |_| {});
        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Configure for Frame {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
