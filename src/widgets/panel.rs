use crate::forest::element::{Configure, Element, LayoutMode};
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::background::Background;
use crate::primitives::transform::TranslateScale;
use crate::ui::Ui;
use crate::widgets::Response;

/// The container widget. Lays children out as `HStack` / `VStack` / `ZStack`
/// (selected via constructor) and optionally paints chrome (via
/// [`Configure::background`]) and/or installs a clip (via
/// [`Configure::clip_rect`] / [`Configure::clip_rounded`]). Cards,
/// rows, columns, and layered overlays all share this one type —
/// `HStack::new()` / `VStack::new()` / `ZStack::new()` just preselect
/// the layout.
///
/// Default chrome / clip is `None`, so a Panel without
/// `.background(...)` / `.clip_*()` paints nothing and doesn't clip
/// — pure layout. The `theme.panel_background` / `theme.panel_clip`
/// fields supply a framework-wide fallback for any panel that didn't
/// set its own.
pub struct Panel {
    element: Element,
    chrome: Option<Background>,
}

impl Panel {
    #[track_caller]
    fn auto(mode: LayoutMode) -> Self {
        Self {
            element: Element::new(mode),
            chrome: None,
        }
    }

    /// Apply a pan/zoom transform to descendants (post-layout). Layout runs
    /// in untransformed space; the transform only affects paint and hit-test.
    /// Composes with any ancestor transform. The panel's *own* background
    /// paints in the parent's space (untransformed) — only children are
    /// transformed.
    pub fn transform(mut self, t: TranslateScale) -> Self {
        self.element.transform = t;
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None` is
    /// the default; theme fallback in [`Self::show`] fills it in from
    /// `ui.theme.panel_background` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;
        // Theme fallback: if the caller left chrome / clip unset,
        // inherit from `theme.panel_*`. Caller intent (any non-None
        // value) wins.
        let mut element = self.element;
        let chrome = self.chrome.or(ui.theme.panel_background);
        if matches!(element.clip, ClipMode::None) {
            element.clip = ui.theme.panel_clip;
        }
        match chrome {
            Some(c) => ui.node_with_chrome(element, c, body),
            None => ui.node(element, body),
        };
        let state = ui.response_for(id);
        Response { id, state }
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
