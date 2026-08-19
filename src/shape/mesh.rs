use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::local_rect_paint_empty;
use crate::shape::sealed;

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
}
// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for MeshShape<'_> {
    fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.tint.is_noop() || self.mesh.is_noop()
    }

    fn lower(self, store: &RecordStore) -> ShapeRecord {
        let Self {
            mesh,
            local_rect,
            tint,
        } = self;
        lower::mesh(store, mesh, local_rect, tint)
    }
}
