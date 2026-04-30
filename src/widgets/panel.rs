use crate::primitives::{Color, Corners, Layout, Sense, Stroke, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutMode;
use crate::ui::Ui;
use crate::widgets::{Layoutable, Response};
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
    id: WidgetId,
    mode: LayoutMode,
    layout: Layout,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
    sense: Sense,
    disabled: bool,
}

impl Panel {
    fn from_id(id: WidgetId, mode: LayoutMode) -> Self {
        Self {
            id,
            mode,
            layout: Layout::default(),
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
            sense: Sense::NONE,
            disabled: false,
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
    /// Make the panel itself an interaction target (clickable card, drag handle, etc).
    /// Default `Sense::NONE` so containers don't intercept clicks meant for children.
    pub fn sense(mut self, s: Sense) -> Self {
        self.sense = s;
        self
    }
    /// Suppress this panel's interactions and cascade to all descendants.
    /// Buttons / clickable widgets nested inside a disabled Panel become
    /// non-interactive without any per-widget API changes. Visual style is
    /// unaffected — apply your own dimming if desired.
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let paints_bg = self.fill.a > 0.0 || self.stroke.is_some();

        let node = ui.node(self.id, self.layout, self.mode, self.sense, |ui| {
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

        if self.disabled {
            ui.tree.node_mut(node).disabled = true;
        }

        let state = ui.response_for(self.id);
        Response { node, state }
    }
}

impl Layoutable for Panel {
    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
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
