use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::transform::TranslateScale;
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::Background;

/// The container widget. Lays children out as `HStack` / `VStack` / `ZStack`
/// (selected via constructor) and optionally paints a background rect
/// (fill / stroke / radius). Cards, rows, columns, and layered overlays all
/// share this one type — `HStack::new()` / `VStack::new()` / `ZStack::new()`
/// just preselect the layout.
///
/// Default fill is transparent and stroke is `None`, so a Panel without
/// `.background(Background { fill: ..., ..Default::default() })` or `.background(Background { stroke: Some(...), ..Default::default() })` paints nothing — pure layout.
pub struct Panel {
    element: Element,
    background: Option<Background>,
}

impl Panel {
    #[track_caller]
    fn auto(mode: LayoutMode) -> Self {
        Self {
            element: Element::new_auto(mode),
            background: None,
        }
    }

    /// Clip descendants' paint to this panel. `Rect` = scissor (cheap);
    /// `Rounded` = stencil mask using the panel's `Background.radius`
    /// (pulls in the stencil render path — apps that never use it pay
    /// nothing). Layout is unchanged — children may still measure beyond.
    pub fn clip(mut self, mode: ClipMode) -> Self {
        self.element.clip = mode;
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

    /// Set the background. Default is `None` — a Panel without this
    /// call paints nothing of its own (pure layout).
    pub fn background(mut self, b: Background) -> Self {
        self.background = Some(b);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;
        // `None` falls back to `theme.panel` (default `None` =
        // pure layout). See `Theme::panel`.
        let bg = self.background.or(ui.theme.panel);

        let node = ui.node(self.element, |ui| {
            if let Some(bg) = bg {
                bg.add_to(ui);
            }
            body(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }

    #[track_caller]
    pub fn hstack() -> Self {
        Self::auto(LayoutMode::HStack)
    }

    #[track_caller]
    pub fn vstack() -> Self {
        Self::auto(LayoutMode::VStack)
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
        Self::auto(LayoutMode::WrapHStack)
    }

    /// VStack with overflow wrap: children flow top-to-bottom; when the
    /// next child wouldn't fit in the current column, wrap to a new
    /// column on the right. Symmetric to `wrap_hstack` — same code,
    /// axes swapped.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::auto(LayoutMode::WrapVStack)
    }

    /// Layered children: each child placed at the parent's inner top-left,
    /// sized per its own `Sizing`. Last sibling paints on top.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::auto(LayoutMode::ZStack)
    }

    /// Children placed at their declared `Layout.position` (parent-inner
    /// coords). Use per-child `.position(Vec2)`. Canvas hugs to the bounding
    /// box of placed children.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::auto(LayoutMode::Canvas)
    }
}

impl Configure for Panel {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
