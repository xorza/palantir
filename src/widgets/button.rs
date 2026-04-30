use crate::primitives::{Color, Corners, Size, Sizes, Spacing, Style, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
use glam::Vec2;
use std::hash::Hash;

pub struct Button {
    id: WidgetId,
    size: Sizes,
    fill: Color,
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
            fill: Color::rgb(0.2, 0.4, 0.8),
            radius: Corners::all(4.0),
            label: String::new(),
        }
    }

    pub fn size(mut self, s: impl Into<Sizes>) -> Self { self.size = s.into(); self }
    pub fn fill(mut self, c: Color) -> Self { self.fill = c; self }
    pub fn radius(mut self, r: impl Into<Corners>) -> Self { self.radius = r.into(); self }
    pub fn label(mut self, s: impl Into<String>) -> Self { self.label = s.into(); self }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = Style {
            size: self.size,
            padding: Spacing::all(8.0),
            ..Default::default()
        };

        let node = ui.node(self.id, style, LayoutKind::Leaf, |ui| {
            ui.add_shape(Shape::RoundedRect {
                bounds: ShapeRect::Full,
                radius: self.radius,
                fill: self.fill,
                stroke: None,
            });

            if !self.label.is_empty() {
                let measured = Size::new(self.label.chars().count() as f32 * 8.0, 16.0);
                ui.add_shape(Shape::Text {
                    offset: Vec2::ZERO,
                    text: self.label,
                    color: Color::WHITE,
                    measured,
                });
            }
        });

        Response { node }
    }
}
