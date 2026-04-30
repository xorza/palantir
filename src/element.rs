use crate::primitives::{Layout, Sense, Size, Sizes, Spacing, WidgetId};
use crate::tree::LayoutMode;
use glam::Vec2;

/// Per-node config bundle: identity + spatial layout + interaction. Every
/// widget builder owns one and forwards it to `Ui::node`. `Element` (the
/// trait below) gives chained setters for all fields by impl'ing one method.
#[derive(Clone, Copy, Debug)]
pub struct UiElement {
    pub id: WidgetId,
    pub layout: Layout,
    pub mode: LayoutMode,
    pub sense: Sense,
    pub disabled: bool,
}

impl UiElement {
    pub fn new(id: WidgetId, mode: LayoutMode) -> Self {
        Self {
            id,
            layout: Layout::default(),
            mode,
            sense: Sense::NONE,
            disabled: false,
        }
    }
}

/// Mixin: any widget builder that holds a `UiElement` gets the chained
/// setters (`.size()`, `.padding()`, `.sense()`, `.disabled()`, …) for
/// free by impl'ing just `element_mut`.
pub trait Element: Sized {
    fn element_mut(&mut self) -> &mut UiElement;

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.element_mut().layout.size = s.into();
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.element_mut().layout.min_size = s.into();
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.element_mut().layout.max_size = s.into();
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.element_mut().layout.padding = p.into();
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.element_mut().layout.margin = m.into();
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout modes.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.element_mut().layout.position = Some(p.into());
        self
    }
    fn sense(mut self, s: Sense) -> Self {
        self.element_mut().sense = s;
        self
    }
    /// Suppress this node's interactions and cascade to all descendants.
    fn disabled(mut self, d: bool) -> Self {
        self.element_mut().disabled = d;
        self
    }
}
