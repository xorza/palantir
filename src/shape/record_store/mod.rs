//! Per-window store for retained record payloads, owned by [`Forest`]. Cleared
//! at record-pass start and retained across `PaintOnly` frames.
//!
//! One store rather than a copy chain: shape records, the encoder's paint
//! payloads and `MeshDraw` entries all carry spans into it directly.
//!
//! This module is storage and the staging calls; deciding *what* to stage
//! (`Shape` to `ShapeRecord` / `ChromeRow`) is [`crate::shape::lower`].
//!
//! [`Forest`]: crate::scene::forest::Forest

pub(crate) mod recorded_gradient;
pub(crate) mod recorded_gradients;
pub(crate) mod text_store;

use crate::common::span::Span;
use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::reduction::Reduction;
use crate::shape::record_store::recorded_gradients::RecordedGradients;
use crate::shape::record_store::text_store::TextStore;
use crate::text::family_keys::FamilyKeys;
use glam::Vec2;

/// The payload columns and the only API that appends to them. Every writer
/// holds `&mut Forest`, so the borrow checker excludes concurrent writes.
#[derive(Default, Debug)]
pub(crate) struct RecordStore {
    /// Vertex storage for `ShapeRecord::Mesh`, indexed by the record's `vertices`. Two columns,
    /// as the polyline payloads are kept, not an authoring `Mesh`, whose running hash and bounds
    /// the arena would leave stale.
    pub(crate) mesh_vertices: Vec<MeshVertex>,
    /// Index storage for `ShapeRecord::Mesh`, indexed by the record's `idxs`.
    pub(crate) mesh_idxs: Vec<u32>,
    /// Point storage for `ShapeRecord::Polyline`, indexed by the record's `points`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`: 1, `points.len()` or
    /// `points.len() - 1` per `ColorMode`, stored as `RgbaF16` (the `CurveInstance`
    /// lane form) so packing happens once at lowering.
    pub(crate) polyline_colors: Vec<RgbaF16>,
    /// Interned record-scoped gradient payloads, indexed by `ShapeBrush::Gradient`
    /// (set by `shape::lower::brush`). Cross-tree, so chrome and shape lowering
    /// share one pool and the encoder needs only this store.
    pub(crate) gradients: RecordedGradients,
    pub(crate) text: TextStore,
    /// The families of the shaper that shapes what this store records, keyed as a text shape
    /// lowers. `Default` gives a table of its own, for a store no shaper reads.
    families: FamilyKeys,
    /// Whether a backdrop filter lowers as a solid fill this pass
    /// ([`crate::Ui::is_transparency_reduced`]).
    pub(crate) transparency: Reduction,
}

/// Where [`RecordStore::stage_mesh`] put one mesh's geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshSpans {
    pub(crate) vertices: Span,
    pub(crate) idxs: Span,
}

/// Where [`RecordStore::stage_polyline`] put one polyline's geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PolylineSpans {
    pub(crate) points: Span,
    pub(crate) colors: Span,
}

impl RecordStore {
    /// A store that records text for the shaper keying families in `families`.
    pub(crate) fn new(families: FamilyKeys) -> Self {
        Self {
            families,
            ..Self::default()
        }
    }

    pub(crate) const fn families(&self) -> &FamilyKeys {
        &self.families
    }

    /// Drop every payload the last record pass appended, keeping capacity.
    /// PaintOnly skips this so the retained tree and storage stay valid together.
    pub(crate) fn clear(&mut self) {
        let Self {
            mesh_vertices,
            mesh_idxs,
            polyline_points,
            polyline_colors,
            gradients,
            text,
            families: _,
            transparency: _,
        } = self;
        mesh_vertices.clear();
        mesh_idxs.clear();
        polyline_points.clear();
        polyline_colors.clear();
        gradients.clear();
        text.clear();
    }

    /// Copy one mesh's vertices and indices in, returning the spans a
    /// `ShapeRecord::Mesh` carries.
    pub(crate) fn stage_mesh(&mut self, mesh: &Mesh) -> MeshSpans {
        let vertices = Span::new(
            self.mesh_vertices.len() as u32,
            mesh.vertices().len() as u32,
        );
        self.mesh_vertices.extend_from_slice(mesh.vertices());
        let idxs = Span::new(self.mesh_idxs.len() as u32, mesh.idxs().len() as u32);
        self.mesh_idxs.extend_from_slice(mesh.idxs());
        MeshSpans { vertices, idxs }
    }

    /// Copy one polyline's points and colors in, each color multiplied by `tint`
    /// and packed once here. Returns the spans a `ShapeRecord::Polyline` carries.
    pub(crate) fn stage_polyline(
        &mut self,
        points: &[Vec2],
        colors: &[RgbaF32],
        tint: RgbaF32,
    ) -> PolylineSpans {
        let staged_points = Span::new(self.polyline_points.len() as u32, points.len() as u32);
        self.polyline_points.extend_from_slice(points);
        let staged_colors = Span::new(self.polyline_colors.len() as u32, colors.len() as u32);
        self.polyline_colors
            .extend(colors.iter().map(|&c| c.tinted(tint).to_f16()));
        PolylineSpans {
            points: staged_points,
            colors: staged_colors,
        }
    }
}

#[cfg(test)]
mod tests;
