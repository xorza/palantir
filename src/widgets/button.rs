use crate::element::{Configure, Element, LayoutMode};
use crate::primitives::{Color, Corners, Sense, Visuals, WidgetId};
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::{Frame, Response, Styled};
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    pub normal: Visuals,
    pub hovered: Visuals,
    pub pressed: Visuals,
    pub disabled: Visuals,
    pub radius: Corners,
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self {
            normal: Visuals::solid(Color::rgb(0.20, 0.40, 0.80), Color::WHITE),
            hovered: Visuals::solid(Color::rgb(0.30, 0.52, 0.92), Color::WHITE),
            pressed: Visuals::solid(Color::rgb(0.10, 0.28, 0.66), Color::WHITE),
            disabled: Visuals::solid(
                Color::rgb(0.22, 0.26, 0.32),
                Color::rgba(1.0, 1.0, 1.0, 0.45),
            ),
            radius: Corners::all(4.0),
        }
    }
}

pub struct Button {
    element: Element,
    style: Option<ButtonStyle>,
    label: String,
}

impl Button {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_id(WidgetId::auto_stable())
    }

    pub fn with_id(id: impl Hash) -> Self {
        let mut element = Element::new(WidgetId::from_hash(id), LayoutMode::Leaf);
        element.sense = Sense::CLICK;
        Self {
            element,
            style: None,
            label: String::new(),
        }
    }

    pub fn style(mut self, s: ButtonStyle) -> Self {
        self.style = Some(s);
        self
    }
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let style = self.style.unwrap_or(ui.theme.button);
        let v = if self.element.disabled {
            style.disabled
        } else {
            let state = ui.response_for(self.element.id);
            if state.pressed {
                style.pressed
            } else if state.hovered {
                style.hovered
            } else {
                style.normal
            }
        };

        let resp = Frame::for_element(self.element)
            .fill(v.fill)
            .stroke(v.stroke)
            .radius(style.radius)
            .show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            ui.tree.add_shape(
                resp.node,
                Shape::Text {
                    text: self.label.clone(),
                    color: v.text,
                    font_size_px: 16.0,
                    wrap: TextWrap::Single,
                },
            );
        }

        resp
    }
}

impl Configure for Button {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
