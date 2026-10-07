//! Triangle-mesh builder; lowers to `ShapeRecord::Mesh`.

use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::record_store::RecordStore;
use crate::shape::lower;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;

/// User-supplied colored triangle mesh.
#[derive(Clone, Debug)]
#[must_use]
pub struct MeshShape<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) tint: RgbaF32,
}

impl<'a> MeshShape<'a> {
    pub(super) const fn new(mesh: &'a Mesh) -> Self {
        Self {
            mesh,
            local_rect: None,
            tint: RgbaF32::WHITE,
        }
    }
}

impl MeshShape<'_> {
    /// Paint into `rect` in owner-relative coords instead of the owner's rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }

    /// Multiplied onto every vertex colour. White leaves the mesh alone.
    ///
    /// # Panics
    ///
    /// Panics unless `tint` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }
}

impl sealed::LowerShape for MeshShape<'_> {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty)
            || self.tint.is_noop()
            || self.mesh.is_noop()
    }

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
