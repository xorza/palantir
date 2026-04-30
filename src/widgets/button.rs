use crate::primitives::{Color, Corners, Size, Sizes, Spacing, Stroke, Style, Visuals, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
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
    id: WidgetId,
    size: Sizes,
    min_size: Size,
    max_size: Size,
    margin: Spacing,
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
        Self {
            id: WidgetId::from_hash(id),
            size: Sizes::HUG,
            min_size: Size::ZERO,
            max_size: Size::INF,
            margin: Spacing::ZERO,
            style: ButtonStyle::default(),
            radius: Corners::all(4.0),
            label: String::new(),
        }
    }

    pub fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.size = s.into();
        self
    }
    pub fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.min_size = s.into();
        self
    }
    pub fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.max_size = s.into();
        self
    }
    pub fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.margin = m.into();
        self
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
        let state = ui.response_for(self.id);
        let v = if state.pressed {
            &self.style.pressed
        } else if state.hovered {
            &self.style.hovered
        } else {
            &self.style.normal
        };

        let style = Style {
            size: self.size,
            min_size: self.min_size,
            max_size: self.max_size,
            padding: Spacing::all(8.0),
            margin: self.margin,
        };

        let node = ui.node(self.id, style, LayoutKind::Leaf, |ui| {
            ui.add_shape(Shape::RoundedRect {
                bounds: ShapeRect::Full,
                radius: self.radius,
                fill: v.fill,
                stroke: v.stroke,
            });

            if !self.label.is_empty() {
                let measured = Size::new(self.label.chars().count() as f32 * 8.0, 16.0);
                ui.add_shape(Shape::Text {
                    offset: Vec2::ZERO,
                    text: self.label.clone(),
                    color: v.text,
                    measured,
                });
            }
        });

        Response { node, state }
    }
}
