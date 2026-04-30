use crate::primitives::{Color, Corners, Sense, Size, Sizes, Spacing, Stroke, Style, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
use glam::Vec2;
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
    kind: LayoutKind,
    size: Sizes,
    min_size: Size,
    max_size: Size,
    padding: Spacing,
    margin: Spacing,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
    sense: Sense,
    disabled: bool,
    position: Option<Vec2>,
}

impl Panel {
    fn from_id(id: WidgetId, kind: LayoutKind) -> Self {
        Self {
            id,
            kind,
            size: Sizes::HUG,
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
            sense: Sense::NONE,
            disabled: false,
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
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout kinds.
    pub fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.position = Some(p.into());
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let style = Style {
            size: self.size,
            min_size: self.min_size,
            max_size: self.max_size,
            padding: self.padding,
            margin: self.margin,
            position: self.position,
        };
        // Skip the bg shape entirely if the panel has nothing to paint — keeps
        // pure-layout HStacks/VStacks zero-shape, like before.
        let paints_bg = self.fill.a > 0.0 || self.stroke.is_some();

        let node = ui.node(self.id, style, self.kind, self.sense, |ui| {
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

pub struct HStack;
pub struct VStack;
pub struct ZStack;
pub struct Canvas;

#[allow(clippy::new_ret_no_self)]
impl HStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutKind::HStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutKind::HStack)
    }
}

#[allow(clippy::new_ret_no_self)]
impl VStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutKind::VStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutKind::VStack)
    }
}

/// Layered children: each child placed at the parent's inner top-left, sized
/// per its own `Sizing`. Last sibling paints on top.
#[allow(clippy::new_ret_no_self)]
impl ZStack {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutKind::ZStack)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutKind::ZStack)
    }
}

/// Children placed at their declared `Style.position` (parent-inner coords).
/// Use per-child `.position(Vec2)`. Canvas hugs to the bounding box of placed
/// children.
#[allow(clippy::new_ret_no_self)]
impl Canvas {
    #[track_caller]
    pub fn new() -> Panel {
        Panel::from_id(WidgetId::auto_stable(), LayoutKind::Canvas)
    }
    pub fn with_id(id: impl Hash) -> Panel {
        Panel::from_id(WidgetId::from_hash(id), LayoutKind::Canvas)
    }
}
