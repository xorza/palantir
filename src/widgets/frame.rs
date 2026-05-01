use crate::element::{Element, LayoutMode, UiElement};
use crate::primitives::{Color, Corners, Stroke, WidgetId};
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

/// A simple decorated rectangle: configurable fill / stroke / radius / size /
/// margin + an optional `Sense`. Used directly for dividers / hit-areas / bg
/// swatches, and as the rendering primitive inside `Button`.
pub struct Frame {
    element: UiElement,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
}

impl Frame {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_element(UiElement::new(WidgetId::auto_stable(), LayoutMode::Leaf))
    }

    pub fn with_id(id: impl Hash) -> Self {
        Self::for_element(UiElement::new(WidgetId::from_hash(id), LayoutMode::Leaf))
    }

    pub fn for_element(element: UiElement) -> Self {
        Self {
            element,
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
        }
    }

    pub fn fill(mut self, c: Color) -> Self {
        self.fill = c;
        self
    }
    /// Accepts `Stroke`, `Option<Stroke>`, or `None`.
    pub fn stroke(mut self, s: impl Into<Option<Stroke>>) -> Self {
        self.stroke = s.into();
        self
    }
    pub fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let node = ui.node(self.element, |ui| {
            ui.add_shape(Shape::RoundedRect {
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
        });

        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Element for Frame {
    fn element_mut(&mut self) -> &mut UiElement {
        &mut self.element
    }
}
