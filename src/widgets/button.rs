use crate::geom::{Color, Size, Sizing, Spacing, Style};
use crate::shape::{Shape, ShapeRect};
use crate::tree::{LayoutKind, NodeId, WidgetId};
use crate::ui::Ui;
use glam::Vec2;
use std::hash::Hash;

pub struct Button {
    id: WidgetId,
    width: Sizing,
    height: Sizing,
    fill: Color,
    radius: f32,
    label: String,
}

pub struct Response {
    pub node: NodeId,
}

impl Button {
    pub fn new(id: impl Hash) -> Self {
        Self {
            id: WidgetId::from_hash(id),
            width: Sizing::Hug,
            height: Sizing::Hug,
            fill: Color::rgb(0.2, 0.4, 0.8),
            radius: 4.0,
            label: String::new(),
        }
    }

    pub fn width(mut self,  v: impl Into<Sizing>) -> Self { self.width  = v.into(); self }
    pub fn height(mut self, v: impl Into<Sizing>) -> Self { self.height = v.into(); self }
    pub fn fill(mut self, c: Color) -> Self { self.fill = c; self }
    pub fn radius(mut self, r: f32) -> Self { self.radius = r; self }
    pub fn label(mut self, s: impl Into<String>) -> Self { self.label = s.into(); self }

    pub fn show(self, ui: &mut Ui) -> Response {
        let style = Style {
            width: self.width,
            height: self.height,
            padding: Spacing::all(8.0),
            ..Default::default()
        };

        let node = ui.begin_node(self.id, style, LayoutKind::Leaf);

        // Background fills the whole node rect.
        ui.add_shape(node, Shape::RoundedRect {
            bounds: ShapeRect::Full,
            radius: self.radius,
            fill: self.fill,
            stroke: None,
        });

        if !self.label.is_empty() {
            // Stub measurement until glyphon is wired up: 8px per char, 16px line.
            let measured = Size::new(self.label.chars().count() as f32 * 8.0, 16.0);
            ui.add_shape(node, Shape::Text {
                offset: Vec2::ZERO,
                text: self.label,
                color: Color::WHITE,
                measured,
            });
        }

        ui.end_node(node);
        Response { node }
    }
}
