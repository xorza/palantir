use crate::primitives::{Color, Corners, Layout, Sense, Stroke, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutMode;
use crate::ui::Ui;
use crate::widgets::{Layoutable, Response};
use std::hash::Hash;

/// A simple decorated rectangle: configurable fill / stroke / radius / size /
/// margin + an optional `Sense`. Used directly for dividers / hit-areas / bg
/// swatches, and as the rendering primitive inside `Button`.
pub struct Frame {
    id: WidgetId,
    layout: Layout,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
    sense: Sense,
}

impl Frame {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_widget_id(WidgetId::auto_stable())
    }

    pub fn with_id(id: impl Hash) -> Self {
        Self::for_widget_id(WidgetId::from_hash(id))
    }

    /// Construct a Frame for an existing `WidgetId` (e.g. a parent widget's id
    /// that wants to reuse Frame's rect-painting machinery without minting a
    /// new id). Used internally by `Button`.
    pub fn for_widget_id(id: WidgetId) -> Self {
        Self {
            id,
            layout: Layout::default(),
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
            sense: Sense::NONE,
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
    pub fn sense(mut self, s: Sense) -> Self {
        self.sense = s;
        self
    }
    pub fn layout(mut self, l: Layout) -> Self {
        self.layout = l;
        self
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let node = ui.node(self.id, self.layout, LayoutMode::Leaf, self.sense, |ui| {
            ui.add_shape(Shape::RoundedRect {
                bounds: ShapeRect::Full,
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
        });

        let state = ui.response_for(self.id);
        Response { node, state }
    }
}

impl Layoutable for Frame {
    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }
}
