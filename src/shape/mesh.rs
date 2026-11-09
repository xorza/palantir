//! Triangle-mesh builder; lowers to `ShapeRecord::Mesh`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::mesh::Mesh;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::paint::point_placement::PointPlacement;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed::{self, Resolve};

/// A colored triangle mesh, its vertices logical px from its rect's top-left: the owner's
/// arranged rect or [`Self::at`] a rect of its own, its vertices fit into it by [`Self::fit`]. A
/// tint that multiplies its colors is its paint ([`Ui::add_shape`](crate::Ui::add_shape)).
#[derive(Clone, Debug)]
#[must_use]
pub struct MeshShape<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) placement: PointPlacement,
}

impl<'a> MeshShape<'a> {
    pub(super) const fn new(mesh: &'a Mesh) -> Self {
        Self {
            mesh,
            placement: PointPlacement::NATURAL,
        }
    }

    /// Lay out in `rect`, in owner-relative coords, instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.placement.local_rect = Some(rect);
        self
    }

    /// Fit the vertices into the rect by `fit`; [`Fit::Natural`], as given, by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement.fit = fit;
        self
    }
}

/// A mesh whose vertex colors a tint multiplies.
impl sealed::LowerShape for (MeshShape<'_>, RgbaF32) {
    /// # Panics
    ///
    /// Unless the tint is a [color](crate::widget::domain::color).
    #[track_caller]
    fn validate(&self) {
        let _ = domain::color(self.1);
    }

    /// Copies vertices and indices into the store and freezes the bbox and content hash.
    fn lower(self, _resolve: Resolve<'_>, store: &mut RecordStore) -> Option<ShapeRecord> {
        let (MeshShape { mesh, placement }, tint) = self;
        if tint.is_noop() || mesh.is_noop() {
            return None;
        }
        let staged = store.stage_mesh(mesh);
        Some(ShapeRecord::Mesh {
            placement,
            tint: tint.to_f16(),
            vertices: staged.vertices,
            idxs: staged.idxs,
            bbox: mesh.bbox(),
            content_hash: mesh.content_hash(),
        })
    }
}
