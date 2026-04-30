use crate::primitives::{Color, Corners, Sense, Size, Sizes, Spacing, Stroke, Style, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

/// Container with a painted background that hugs its children. Children are
/// laid out as a ZStack — all positioned at the top-left of the panel's
/// inner rect, sized per their own `Sizing`. Useful for cards, modals, or
/// any "background + content" composition.
///
/// For absolute-positioned children, see (future) `Canvas` widget.
pub struct Panel {
    id: WidgetId,
    size: Sizes,
    min_size: Size,
    max_size: Size,
    padding: Spacing,
    margin: Spacing,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
    sense: Sense,
}

impl Panel {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_widget_id(WidgetId::auto_stable())
    }

    pub fn with_id(id: impl Hash) -> Self {
        Self::for_widget_id(WidgetId::from_hash(id))
    }

    pub fn for_widget_id(id: WidgetId) -> Self {
        Self {
            id,
            size: Sizes::HUG,
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
            sense: Sense::NONE,
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
    pub fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.padding = p.into();
        self
    }
    pub fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.margin = m.into();
        self
    }
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = c;
        self
    }
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

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let style = Style {
            size: self.size,
            min_size: self.min_size,
            max_size: self.max_size,
            padding: self.padding,
            margin: self.margin,
        };

        let node = ui.node(self.id, style, LayoutKind::ZStack, self.sense, |ui| {
            ui.add_shape(Shape::RoundedRect {
                bounds: ShapeRect::Full,
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
            body(ui);
        });

        let state = ui.response_for(self.id);
        Response { node, state }
    }
}
