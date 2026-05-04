use crate::element::{Configure, Element, LayoutMode};
use crate::primitives::{transform::TranslateScale, widget_id::WidgetId};
use crate::ui::Ui;
use crate::widgets::{Response, styled::Background, styled::Styled};
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
    element: Element,
    background: Background,
}

impl Panel {
    fn from_id(id: WidgetId, mode: LayoutMode) -> Self {
        Self {
            element: Element::new(id, mode),
            background: Background::default(),
        }
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
            self.background.add_to(ui);
            body(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }

    #[track_caller]
    pub fn hstack() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::HStack)
    }
    pub fn hstack_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::HStack)
    }

    #[track_caller]
    pub fn vstack() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::VStack)
    }
    pub fn vstack_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::VStack)
    }

    /// HStack with overflow wrap: children flow left-to-right; when the
    /// next child wouldn't fit on the current row, wrap to a new row
    /// below. `.gap(g)` spaces siblings within a row; `.line_gap(g)`
    /// spaces rows. `.justify(...)` applies per row.
    /// `Sizing::Fill` on a child's main axis is treated as `Hug` for
    /// now (no per-row leftover distribution); cross-axis Fill stretches
    /// to row height.
    #[track_caller]
    pub fn wrap_hstack() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::WrapHStack)
    }
    pub fn wrap_hstack_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::WrapHStack)
    }

    /// VStack with overflow wrap: children flow top-to-bottom; when the
    /// next child wouldn't fit in the current column, wrap to a new
    /// column on the right. Symmetric to `wrap_hstack` — same code,
    /// axes swapped.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::WrapVStack)
    }
    pub fn wrap_vstack_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::WrapVStack)
    }

    /// Layered children: each child placed at the parent's inner top-left,
    /// sized per its own `Sizing`. Last sibling paints on top.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::ZStack)
    }
    pub fn zstack_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::ZStack)
    }

    /// Children placed at their declared `Layout.position` (parent-inner
    /// coords). Use per-child `.position(Vec2)`. Canvas hugs to the bounding
    /// box of placed children.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::from_id(WidgetId::auto_stable(), LayoutMode::Canvas)
    }
    pub fn canvas_with_id(id: impl Hash) -> Self {
        Self::from_id(WidgetId::from_hash(id), LayoutMode::Canvas)
    }
}

impl Configure for Panel {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

impl Styled for Panel {
    fn background_mut(&mut self) -> &mut Background {
        &mut self.background
    }
}
