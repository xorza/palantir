use crate::element::{Element, UiElement};
use crate::primitives::{Color, Corners, Stroke, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutMode;
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

/// The container widget. Lays children out as `HStack` / `VStack` / `ZStack`
/// (selected via constructor) and optionally paints a background rect
/// (fill / stroke / radius). Cards, rows, columns, and layered overlays all
/// share this one type — `HStack::new()` / `VStack::new()` / `ZStack::new()`
/// just preselect the layout.
///
/// Default fill is transparent and stroke is `None`, so a Panel without
/// `.fill(...)` or `.stroke(...)` paints nothing — pure layout.
pub struct Panel {
    element: UiElement,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
}

impl Panel {
    fn from_id(id: WidgetId, mode: LayoutMode) -> Self {
        Self {
            element: UiElement::new(id, mode),
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
        }
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

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let paints_bg = self.fill.a > 0.0 || self.stroke.is_some();
        let id = self.element.id;

        let node = ui.node(self.element, |ui| {
            if paints_bg {
                ui.add_shape(Shape::RoundedRect {
                    bounds: ShapeRect::Full,
                    radius: self.radius,
                    fill: self.fill,
                    stroke: self.stroke,
                });
            }
            body(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Element for Panel {
    fn element_mut(&mut self) -> &mut UiElement {
        &mut self.element
    }
}

pub struct HStack;
pub struct VStack;
pub struct ZStack;
pub struct Canvas;

#[allow(clippy::new_ret_no_self)]
impl HStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutMode::HStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutMode::HStack)
    }
}

#[allow(clippy::new_ret_no_self)]
impl VStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutMode::VStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutMode::VStack)
    }
}

/// Layered children: each child placed at the parent's inner top-left, sized
/// per its own `Sizing`. Last sibling paints on top.
#[allow(clippy::new_ret_no_self)]
impl ZStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutMode::ZStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutMode::ZStack)
    }
}

/// Children placed at their declared `Layout.position` (parent-inner coords).
/// Use per-child `.position(Vec2)`. Canvas hugs to the bounding box of placed
/// children.
#[allow(clippy::new_ret_no_self)]
impl Canvas {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutMode::Canvas)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutMode::Canvas)
    }
}
