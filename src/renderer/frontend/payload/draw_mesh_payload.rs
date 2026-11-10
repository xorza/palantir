//! One indexed-triangle mesh draw.

use crate::common::span::Span;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::spin::Spin;
use glam::Vec2;

/// Mesh draw payload: spans into the window's [`RecordStore`] (`meshes`) of vertices as recorded;
/// the composer folds `fit` and `origin` into the per-instance transform, so the vertex stream
/// stays content-stable.
///
/// [`RecordStore`]: crate::shape::record_store::RecordStore
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawMeshPayload {
    /// Owner-local AABB of `vertices`, fitted.
    pub(crate) bbox: Rect,
    pub(crate) origin: Vec2,
    /// Maps a vertex into the owner-local frame.
    pub(crate) fit: FitMap,
    pub(crate) tint: RgbaF16,
    pub(crate) vertices: Span,
    pub(crate) idxs: Span,
    /// A paint animation's turn.
    pub(crate) spin: Option<Spin>,
}

impl PaintPayload for DrawMeshPayload {
    /// This draw with alpha scaled by `by`, for the [`PaintSink`] gate.
    #[inline]
    fn faded(self, by: f32) -> Self {
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
    fn is_noop(&self) -> bool {
        self.vertices.len == 0
            || self.idxs.len < 3
            || !self.idxs.len.is_multiple_of(3)
            || self.tint.is_noop()
    }

    fn spin_mut(&mut self) -> &mut Option<Spin> {
        &mut self.spin
    }

    fn paint(self, sink: &mut impl PaintSink) {
        sink.mesh(self);
    }
}
