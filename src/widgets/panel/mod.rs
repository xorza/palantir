//! The container widget — every stack, wrap and canvas layout an app
//! reaches for, over the one node the layout drivers dispatch on.

use crate::primitives::background::Background;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widgets::response::InnerResponse;
use std::rc::Rc;

/// The container widget. Lays children out as `HStack` / `VStack` / `ZStack`
/// (selected via constructor) and optionally paints chrome (via
/// [`Self::background`]) and/or installs a clip (via
/// [`Configure::clip_rect`](crate::Configure::clip_rect) / [`Configure::clip_rounded`](crate::Configure::clip_rounded)). Cards,
/// rows, columns, and layered overlays all share this one type —
/// `HStack::new()` / `VStack::new()` / `ZStack::new()` just preselect
/// the layout.
///
/// Default chrome / clip is `None`, so a Panel without
/// `.background(...)` / `.clip_*()` paints nothing and doesn't clip
/// — pure layout. The `theme.panel_background` / `theme.panel_clip`
/// fields supply a framework-wide fallback for any panel that didn't
/// set its own.
#[derive(Debug)]
pub struct Panel {
    node: Node,
    chrome: Option<Background>,
}

impl Panel {
    fn auto(node: Node) -> Self {
        Self { node, chrome: None }
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        // Theme fallback: if the caller left chrome / clip unset,
        // inherit from `theme.panel_*`. Caller intent (any non-None
        // value) wins.
        // The theme handle is cloned, not the chrome: an `Rc` bump lets
        // the borrow outlive the `&mut Ui` the record below takes.
        let theme = Rc::clone(ui.theme());
        let mut node = self.node;
        let chrome = node.resolve_container_chrome(
            self.chrome.as_ref(),
            theme.panel_background.as_ref(),
            theme.panel_clip,
        );
        ui.widget(node).show(ui, chrome, body)
    }

    #[track_caller]
    pub fn hstack() -> Self {
        Self::auto(Node::hstack())
    }

    #[track_caller]
    pub fn vstack() -> Self {
        Self::auto(Node::vstack())
    }

    /// HStack with overflow wrap: children flow left-to-right; when the
    /// next child wouldn't fit on the current row, wrap to a new row
    /// below. `.gap(g)` spaces siblings within a row; `.line_gap(g)`
    /// spaces rows. `.justify(...)` applies per row.
    /// `Sizing::fill` on a child's main axis is treated as `Hug` for
    /// now (no per-row leftover distribution); cross-axis Fill stretches
    /// to row height.
    #[track_caller]
    pub fn wrap_hstack() -> Self {
        Self::auto(Node::wrap_hstack())
    }

    /// VStack with overflow wrap: children flow top-to-bottom; when the
    /// next child wouldn't fit in the current column, wrap to a new
    /// column on the right. Symmetric to `wrap_hstack` — same code,
    /// axes swapped.
    #[track_caller]
    pub fn wrap_vstack() -> Self {
        Self::auto(Node::wrap_vstack())
    }

    /// Layered children: each child placed at the parent's inner top-left,
    /// sized per its own `Sizing`. Last sibling paints on top.
    #[track_caller]
    pub fn zstack() -> Self {
        Self::auto(Node::zstack())
    }

    /// Children placed at their declared `Layout.position` (parent-inner
    /// coords). Use per-child `.position(Vec2)`. Canvas hugs to the bounding
    /// box of placed children.
    #[track_caller]
    pub fn canvas() -> Self {
        Self::auto(Node::canvas())
    }
}

impl_background!(
    Panel,
    "`None` is the default; theme fallback in [`Self::show`] fills it in from \
     `ui.theme().panel_background` when unset. Pass [`Background::NONE`] to \
     suppress that fallback for this panel.",
);
impl_configure!(Panel);

#[cfg(test)]
mod tests;
