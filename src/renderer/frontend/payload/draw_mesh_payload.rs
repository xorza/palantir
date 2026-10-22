//! One indexed-triangle mesh draw.

use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use glam::Vec2;

/// Mesh draw payload: owner-local spans into the window's [`RecordStore`] (`meshes`); the composer folds `origin` into the per-instance translate so the vertex stream stays content-stable.
///
/// [`RecordStore`]: crate::shape::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawMeshPayload {
    /// Owner-local AABB of `vertices`.
    pub(crate) bbox: Rect,
    pub(crate) origin: Vec2,
    pub(crate) tint: RgbaF16,
    pub(crate) vertices: Span,
    pub(crate) idxs: Span,
}

impl DrawMeshPayload {
    /// This draw with alpha scaled by `by`, for the [`PaintSink`](crate::renderer::frontend::paint_sink::PaintSink) gate.
    #[inline]
    pub(crate) fn faded(self, by: f32) -> Self {
        if by == 1.0 {
            return self;
        }
        Self {
            tint: self.tint.faded(by),
            ..self
        }
    }

    /// Paints nothing with empty vertices, under one triangle, a non-multiple-of-3 index count, or a transparent tint.
    #[inline]
    pub(crate) const fn is_noop(&self) -> bool {
        self.vertices.len == 0
            || self.idxs.len < 3
            || !self.idxs.len.is_multiple_of(3)
            || self.tint.is_noop()
    }
}
