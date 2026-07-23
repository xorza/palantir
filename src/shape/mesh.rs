use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::shape::local_rect_paint_empty;

/// User-supplied colored triangle mesh.
#[derive(Clone, Debug)]
pub struct MeshShape<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) tint: Color,
}

impl MeshShape<'_> {
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn tint(mut self, tint: impl Into<Color>) -> Self {
        self.tint = tint.into();
        self
    }

    pub(crate) fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.tint.is_noop() || self.mesh.is_noop()
    }
}
