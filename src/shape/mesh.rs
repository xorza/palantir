//! The triangle-mesh builder. Lowers to `ShapeRecord::Mesh`, with the
//! vertices and indices copied into the record store.

use crate::primitives::color::RgbaF32;
use crate::primitives::mesh::Mesh;
use crate::primitives::nan::NanCheck;
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
    pub(crate) tint: RgbaF32,
}

impl<'a> MeshShape<'a> {
    pub(super) fn new(mesh: &'a Mesh) -> Self {
        Self {
            mesh,
            local_rect: None,
            tint: RgbaF32::WHITE,
        }
    }
}

impl MeshShape<'_> {
    /// Paint into `rect`, in owner-relative coords, instead of the
    /// owner's whole arranged rect.
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn tint(mut self, tint: impl Into<RgbaF32>) -> Self {
        self.tint = tint.into();
        self
    }
}

impl sealed::LowerShape for MeshShape<'_> {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
            || self.tint.is_noop()
            || self.mesh.is_noop()
    }

    /// The vertices reach this as the memoized bbox, so the whole shape
    /// is one load and three tests. The two placement fields are what
    /// only this screen sees: lowering copies every vertex and index into
    /// the store before a record exists to be judged on them.
    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.tint.has_nan() || self.mesh.bbox().has_nan()
    }

    fn lower(self, store: &mut RecordStore) -> ShapeRecord {
        let Self {
            mesh,
            local_rect,
            tint,
        } = self;
        lower::mesh(store, mesh, local_rect, tint)
    }
}
