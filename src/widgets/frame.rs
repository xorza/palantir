use crate::tree::element::{Configure, Element, LayoutMode};
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::{Response, styled::Background, styled::Styled};
use std::hash::Hash;

/// A simple decorated rectangle: configurable fill / stroke / radius / size /
/// margin + an optional `Sense`. Used directly for dividers / hit-areas / bg
/// swatches, and as the rendering primitive inside `Button`.
pub struct Frame {
    element: Element,
    background: Background,
}

impl Frame {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_element(Element::new(WidgetId::auto_stable(), LayoutMode::Leaf))
    }

    pub fn with_id(id: impl Hash) -> Self {
        Self::for_element(Element::new(WidgetId::from_hash(id), LayoutMode::Leaf))
    }

    pub fn for_element(element: Element) -> Self {
        Self {
            element,
            background: Background::default(),
        }
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let node = ui.node(self.element, |ui| {
            self.background.add_to(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Configure for Frame {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

impl Styled for Frame {
    fn background_mut(&mut self) -> &mut Background {
        &mut self.background
    }
}
