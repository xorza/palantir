use crate::element::{Element, LayoutMode, UiElement};
use crate::primitives::{Color, Corners, Stroke, TranslateScale, WidgetId};
use crate::shape::Shape;
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
    /// Clip descendants' paint to this panel's rendered rect (CSS
    /// `overflow: hidden`). Layout is unchanged — children may still measure
    /// beyond, they're just visually scissored.
    pub fn clip(mut self, c: bool) -> Self {
        self.element.clip = c;
        self
    }
    /// Apply a pan/zoom transform to descendants (post-layout). Layout runs
    /// in untransformed space; the transform only affects paint and hit-test.
    /// Composes with any ancestor transform. The panel's *own* background
    /// paints in the parent's space (untransformed) — only children are
    /// transformed.
    pub fn transform(mut self, t: TranslateScale) -> Self {
        self.element.transform = Some(t);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;

        let node = ui.node(self.element, |ui| {
            ui.add_shape(Shape::RoundedRect {
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
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
