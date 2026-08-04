use crate::primitives::color::Color;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::scene::record_store::RecordStore;
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
        let mut payloads = store.payloads.borrow_mut();
        let v_start = payloads.meshes.vertices.len() as u32;
        payloads
            .meshes
            .vertices
            .extend_from_slice(&self.mesh.vertices);
        let i_start = payloads.meshes.indices.len() as u32;
        payloads
            .meshes
            .indices
            .extend_from_slice(&self.mesh.indices);
        ShapeRecord::Mesh {
            local_rect: self.local_rect,
            tint: self.tint.into(),
            vertices: Span::new(v_start, self.mesh.vertices.len() as u32),
            indices: Span::new(i_start, self.mesh.indices.len() as u32),
            bbox: self.mesh.bbox(),
            content_hash: self.mesh.content_hash(),
        }
    }
}
