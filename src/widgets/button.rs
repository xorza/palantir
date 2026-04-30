use crate::element::{Element, UiElement};
use crate::primitives::{Color, Corners, Sense, Spacing, Stroke, Visuals, WidgetId};
use crate::shape::Shape;
use crate::tree::LayoutMode;
use crate::ui::Ui;
use crate::widgets::{Frame, Response};
use glam::Vec2;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonStyle {
    pub normal: Visuals,
    pub hovered: Visuals,
    pub pressed: Visuals,
}

impl ButtonStyle {
    pub const fn uniform(v: Visuals) -> Self {
        Self {
            normal: v,
            hovered: v,
            pressed: v,
        }
    }

    pub fn primary() -> Self {
        Self {
            normal: Visuals::solid(Color::rgb(0.20, 0.40, 0.80), Color::WHITE),
            hovered: Visuals::solid(Color::rgb(0.30, 0.52, 0.92), Color::WHITE),
            pressed: Visuals::solid(Color::rgb(0.10, 0.28, 0.66), Color::WHITE),
        }
    }

    pub fn outlined() -> Self {
        let stroke = Some(Stroke {
            width: 1.5,
            color: Color::rgb(0.4, 0.5, 0.7),
        });
        Self {
            normal: Visuals {
                fill: Color::TRANSPARENT,
                stroke,
                text: Color::rgb(0.85, 0.88, 0.95),
            },
            hovered: Visuals {
                fill: Color::rgba(0.4, 0.5, 0.7, 0.15),
                stroke,
                text: Color::WHITE,
            },
            pressed: Visuals {
                fill: Color::rgba(0.4, 0.5, 0.7, 0.30),
                stroke,
                text: Color::WHITE,
            },
        }
    }
}

impl Default for ButtonStyle {
    fn default() -> Self {
        Self::primary()
    }
}

pub struct Button {
    element: UiElement,
    style: ButtonStyle,
    radius: Corners,
    label: String,
}

impl Button {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_id(WidgetId::auto_stable())
    }

    pub fn with_id(id: impl Hash) -> Self {
        let mut element = UiElement::new(WidgetId::from_hash(id), LayoutMode::Leaf);
        element.layout.padding = Spacing::all(8.0);
        element.sense = Sense::CLICK;
        Self {
            element,
            style: ButtonStyle::default(),
            radius: Corners::all(4.0),
            label: String::new(),
        }
    }

    pub fn style(mut self, s: ButtonStyle) -> Self {
        self.style = s;
        self
    }
    pub fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self
    }
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = s.into();
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let v = {
            let state = ui.response_for(self.element.id);
            if state.pressed {
                self.style.pressed
            } else if state.hovered {
                self.style.hovered
            } else {
                self.style.normal
            }
        };

        let resp = Frame::for_element(self.element)
            .fill(v.fill)
            .stroke(v.stroke)
            .radius(self.radius)
            .show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            let measured =
                crate::primitives::Size::new(self.label.chars().count() as f32 * 8.0, 16.0);
            ui.tree.add_shape(
                resp.node,
                Shape::Text {
                    offset: Vec2::ZERO,
                    text: self.label.clone(),
                    color: v.text,
                    measured,
                },
            );
        }

        resp
    }
}

impl Element for Button {
    fn element_mut(&mut self) -> &mut UiElement {
        &mut self.element
    }
}
