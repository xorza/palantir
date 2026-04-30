use crate::primitives::{Color, Corners, Sense, Size, Sizes, Spacing, Stroke, Visuals, WidgetId};
use crate::shape::Shape;
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
    id: WidgetId,
    size: Sizes,
    min_size: Size,
    max_size: Size,
    margin: Spacing,
    style: ButtonStyle,
    radius: Corners,
    label: String,
    position: Option<Vec2>,
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
            position: None,
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
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout kinds.
    pub fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.position = Some(p.into());
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let v = {
            let state = ui.response_for(self.id);
            if state.pressed {
                self.style.pressed
            } else if state.hovered {
                self.style.hovered
            } else {
                self.style.normal
            }
        };

        let mut frame = Frame::for_widget_id(self.id)
            .size(self.size)
            .min_size(self.min_size)
            .max_size(self.max_size)
            .padding(Spacing::all(8.0))
            .margin(self.margin)
            .fill(v.fill)
            .stroke(v.stroke)
            .radius(self.radius)
            .sense(Sense::CLICK);
        if let Some(p) = self.position {
            frame = frame.position(p);
        }
        let resp = frame.show(ui);

        // Layer the label on top of the frame's background. Safe immediately after
        // `Frame::show` because no other shape/node has been pushed since the
        // frame closed — `Tree::add_shape`'s contiguity invariant still holds.
        if !self.label.is_empty() {
            let measured = Size::new(self.label.chars().count() as f32 * 8.0, 16.0);
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
