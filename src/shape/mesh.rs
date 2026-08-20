use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::lower;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;

/// User-supplied colored triangle mesh.
#[derive(Clone, Debug)]
pub struct MeshShape<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) tint: Color,
}

local_rect_shape!(MeshShape<'_>);

shape_setters!(MeshShape<'_> {
    tint: Color => tint,
});

impl sealed::LowerShape for MeshShape<'_> {
    fn is_noop(&self) -> bool {
        self.rect_is_noop() || self.tint.is_noop() || self.mesh.is_noop()
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
