use crate::primitives::{Layout, Size, Sizes, Spacing};
use glam::Vec2;

/// Layout-builder mixin: any widget that holds a `Layout` gets the chained
/// setters (`.size()`, `.padding()`, `.position()`, …) for free by impl'ing
/// just `layout_mut`.
pub trait Layoutable: Sized {
    fn layout_mut(&mut self) -> &mut Layout;

    fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.layout_mut().size = s.into();
        self
    }
    fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.layout_mut().min_size = s.into();
        self
    }
    fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.layout_mut().max_size = s.into();
        self
    }
    fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.layout_mut().padding = p.into();
        self
    }
    fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.layout_mut().margin = m.into();
        self
    }
    /// Absolute position inside a `Canvas` parent (parent-inner coords).
    /// Ignored by other layout kinds.
    fn position(mut self, p: impl Into<Vec2>) -> Self {
        self.layout_mut().position = Some(p.into());
        self
    }
}
